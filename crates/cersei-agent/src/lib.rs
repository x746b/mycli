//! cersei-agent: The high-level Agent API with builder pattern, agentic loop,
//! realtime event streaming, broadcast channels, and reporters.

pub mod agent_tool;
pub mod auto_dream;
pub mod compact;
pub mod context_analyzer;
pub mod coordinator;
pub mod effort;
pub mod events;
pub mod reporters;
pub mod session_memory;
pub mod system_prompt;
mod runner;

// Re-export runner utilities
pub use runner::apply_tool_result_budget;

use cersei_hooks::Hook;
use cersei_memory::Memory;
use cersei_mcp::McpServerConfig;
use cersei_provider::Provider;
use cersei_tools::permissions::{AllowAll, PermissionPolicy};
use cersei_tools::{CostTracker, Tool};
use cersei_types::*;
use events::AgentEvent;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};

// Re-exports
pub use events::{AgentStream, CompactReason, WarningState};
pub use reporters::Reporter;

// ─── Agent output ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AgentOutput {
    pub message: Message,
    pub usage: Usage,
    pub stop_reason: StopReason,
    pub turns: u32,
    pub tool_calls: Vec<ToolCallRecord>,
}

impl AgentOutput {
    pub fn text(&self) -> &str {
        self.message.get_text().unwrap_or("")
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub name: String,
    pub id: String,
    pub input: serde_json::Value,
    pub result: String,
    pub is_error: bool,
    pub duration: Duration,
}

// ─── Agent ───────────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub struct Agent {
    provider: Box<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    model: Option<String>,
    max_turns: u32,
    max_tokens: u32,
    temperature: Option<f32>,
    top_p: Option<f32>,
    min_p: Option<f32>,
    thinking_budget: Option<u32>,
    /// Model-level reasoning switch. `None` leaves it to the model's default;
    /// `Some(false)` asks the provider to turn reasoning off outright, which is
    /// different from merely not displaying it.
    thinking_enabled: Option<bool>,
    reasoning_effort: Option<String>,
    working_dir: PathBuf,
    permission_policy: Arc<dyn PermissionPolicy>,
    memory: Option<Arc<dyn Memory>>,
    session_id: Option<String>,
    hooks: Vec<Arc<dyn Hook>>,
    mcp_manager: Option<Arc<cersei_mcp::McpManager>>,
    event_handler: Option<Box<dyn Fn(&AgentEvent) + Send + Sync>>,
    broadcast_tx: Option<broadcast::Sender<AgentEvent>>,
    reporters: Vec<Arc<dyn Reporter>>,
    event_filter: Option<Box<dyn Fn(&AgentEvent) -> bool + Send + Sync>>,
    cost_tracker: Arc<CostTracker>,
    auto_compact: bool,
    context_state: parking_lot::Mutex<compact::ContextState>,
    compact_threshold: f64,
    tool_result_budget: usize,
    messages: Arc<parking_lot::Mutex<Vec<Message>>>,
    cumulative_usage: Arc<parking_lot::Mutex<Usage>>,
    cancel_token: tokio_util::sync::CancellationToken,
    pub(crate) context_window: u64,
}

impl Agent {
    /// Tokens this model can hold, as stated by the provider when it says and
    /// guessed from the model name otherwise.
    pub fn context_window(&self) -> u64 {
        self.context_window
    }

    pub(crate) fn generation_options(&self) -> cersei_provider::ProviderOptions {
        let mut options = cersei_provider::ProviderOptions::default();
        if let Some(value) = self.top_p { options.set("top_p", value); }
        if let Some(value) = self.min_p { options.set("min_p", value); }
        if let Some(value) = &self.reasoning_effort { options.set("reasoning_effort", value); }
        if let Some(value) = self.thinking_budget { options.set("thinking_budget", value); }
        if let Some(value) = self.thinking_enabled { options.set("thinking", value); }
        options
    }

    pub fn auto_compaction_paused(&self) -> bool {
        self.context_state.lock().compact.disabled
    }

    pub fn context_input_budget(&self) -> u64 {
        self.context_window.saturating_sub(u64::from(self.max_tokens) + 1024)
    }

    pub fn context_estimate(&self) -> u64 {
        let mut messages = self.messages();
        runner::apply_tool_result_budget(&mut messages, self.tool_result_budget);
        let tools: Vec<_> = self.tools.iter().map(|t| t.to_definition()).collect();
        self.context_state.lock().estimate(runner::input_size(&messages, &tools, self.system_prompt.as_deref()))
    }

    /// Summarize older complete turns, preserving the latest user turn and tool pairs.
    pub async fn compact(&mut self, instructions: Option<&str>) -> Result<compact::CompactResult> {
        self.compact_history(2, instructions).await
    }

    pub(crate) async fn compact_history(&self, keep_recent: usize, instructions: Option<&str>) -> Result<compact::CompactResult> {
        let original = self.messages();
        let tail = &original[original.len().saturating_sub(keep_recent)..];
        let usable = self.context_window.saturating_sub(u64::from(self.max_tokens) + 1024);
        let keep_recent = if runner::input_size(tail, &[], None) as u64 > usable / 2 { 2 } else { keep_recent };
        let mut usage = Usage::default();
        let result = tokio::select! {
            biased;
            _ = self.cancel_token.cancelled() => Err(CerseiError::Cancelled),
            result = compact::compact_with_options(self.provider.as_ref(), &original,
                self.model.as_deref().unwrap_or(""), keep_recent, instructions, self.context_window,
                self.max_tokens, self.generation_options(), self.temperature, &mut usage) => result,
        };
        self.cumulative_usage.lock().merge(&usage);
        self.cost_tracker.add(&usage);
        let result = result?;
        if self.cancel_token.is_cancelled() { return Err(CerseiError::Cancelled); }
        if !result.messages.is_empty() {
            if let (Some(memory), Some(session)) = (&self.memory, &self.session_id) {
                memory.checkpoint(session, &result.messages, &self.usage()).await?;
            }
            *self.messages.lock() = result.messages.clone();
            let mut state = self.context_state.lock();
            state.measured_tokens = 0;
            state.measured_bytes = 0;
            state.compact.on_success();
        }
        Ok(result)
    }

    /// Attach explicitly selected session storage and its already-loaded context.
    pub fn attach_session(&mut self, memory: Arc<dyn cersei_memory::Memory>, id: String, messages: Vec<Message>, usage: Usage) {
        self.memory = Some(memory);
        self.session_id = Some(id);
        *self.messages.lock() = messages;
        self.cost_tracker.add(&usage);
        *self.cumulative_usage.lock() = usage;
        *self.context_state.lock() = Default::default();
        self.repair_interrupted_tools();
    }

    pub fn set_system_prompt(&mut self, prompt: String) {
        if self.system_prompt.as_deref() != Some(&prompt) {
            self.system_prompt = Some(prompt);
            self.context_state.lock().measured_tokens = 0;
        }
    }

    pub(crate) async fn persist_context(&self) -> Result<()> {
        if let (Some(memory), Some(id)) = (&self.memory, &self.session_id) {
            let messages = self.messages();
            let usage = self.usage();
            memory.checkpoint(id, &messages, &usage).await?;
        }
        Ok(())
    }

    /// Interrupted tools have unknown outcomes. Record that fact, never replay them.
    pub(crate) fn repair_interrupted_tools(&self) {
        let mut messages = self.messages.lock();
        let mut pending: Vec<String> = Vec::new();
        for message in messages.iter() {
            for block in message.content_blocks() {
                match block {
                    ContentBlock::ToolUse { id, .. } => pending.push(id),
                    ContentBlock::ToolResult { tool_use_id, .. } => pending.retain(|id| *id != tool_use_id),
                    _ => {}
                }
            }
        }
        if !pending.is_empty() {
            messages.push(Message::user_blocks(pending.into_iter().map(|id| ContentBlock::ToolResult {
                tool_use_id: id, is_error: Some(true),
                content: ToolResultContent::Text("Tool execution was interrupted; outcome is unknown. Verify the current state before retrying any action.".into()),
            }).collect()));
        }
    }

    pub fn builder() -> AgentBuilder {
        AgentBuilder::default()
    }

    /// Run a prompt through the agentic loop.
    pub async fn run(&self, prompt: &str) -> cersei_types::Result<AgentOutput> {
        runner::run_agent(self, prompt).await
    }

    /// Run with streaming — returns a stream of AgentEvents.
    pub fn run_stream(&self, prompt: &str) -> AgentStream {
        let (event_tx, event_rx) = mpsc::channel(512);
        let (control_tx, control_rx) = mpsc::channel(64);

        let prompt = prompt.to_string();
        let agent_ptr = unsafe {
            // SAFETY: Agent is borrowed for the duration of the spawned task.
            // In a real implementation, Agent would be Arc-wrapped.
            &*(self as *const Agent)
        };

        tokio::spawn(async move {
            let result = runner::run_agent_streaming(agent_ptr, &prompt, event_tx.clone(), control_rx).await;
            match result {
                Ok(output) => {
                    let _ = event_tx.send(AgentEvent::Complete(output)).await;
                }
                Err(e) => {
                    let _ = event_tx.send(AgentEvent::Error(e.to_string())).await;
                }
            }
        });

        AgentStream::new(event_rx, control_tx)
    }

    /// Multi-turn: send a follow-up message in the same conversation.
    pub async fn reply(&self, message: &str) -> cersei_types::Result<AgentOutput> {
        runner::run_agent(self, message).await
    }

    /// Access the conversation history.
    pub fn messages(&self) -> Vec<Message> {
        self.messages.lock().clone()
    }

    /// Get cumulative usage/cost.
    pub fn usage(&self) -> Usage {
        self.cumulative_usage.lock().clone()
    }

    /// Cancel a running agent.
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Install a fresh cancellation token, arming the agent for another turn.
    ///
    /// A `CancellationToken` is one-shot: once tripped it stays tripped. A
    /// long-lived agent whose turn was interrupted would otherwise refuse
    /// every later turn, so a caller that supports interruption must re-arm
    /// between turns.
    pub fn set_cancel_token(&mut self, token: tokio_util::sync::CancellationToken) {
        self.cancel_token = token;
    }

    /// Switch model-level reasoning at runtime, so a REPL can toggle it between
    /// turns without rebuilding the agent.
    pub fn set_thinking_enabled(&mut self, on: bool) {
        self.thinking_enabled = Some(on);
        self.context_state.lock().measured_tokens = 0;
    }

    pub fn thinking_enabled(&self) -> Option<bool> {
        self.thinking_enabled
    }

    /// Change effort between turns without resetting conversation history.
    pub fn set_reasoning_effort(&mut self, effort: Option<String>) {
        self.reasoning_effort = effort;
        self.context_state.lock().measured_tokens = 0;
    }

    /// Subscribe to the broadcast channel (requires enable_broadcast on builder).
    pub fn subscribe(&self) -> Option<broadcast::Receiver<AgentEvent>> {
        self.broadcast_tx.as_ref().map(|tx| tx.subscribe())
    }

    /// Emit an event to all listeners.
    pub(crate) fn emit(&self, event: AgentEvent) {
        // Apply filter
        if let Some(filter) = &self.event_filter {
            if !filter(&event) {
                return;
            }
        }

        // Callback handler
        if let Some(handler) = &self.event_handler {
            handler(&event);
        }

        // Broadcast channel
        if let Some(tx) = &self.broadcast_tx {
            let _ = tx.send(event.clone());
        }

        // Reporters
        for reporter in &self.reporters {
            let reporter = Arc::clone(reporter);
            let event = event.clone();
            tokio::spawn(async move {
                reporter.on_event(&event).await;
            });
        }
    }
}

// ─── Agent builder ───────────────────────────────────────────────────────────

pub struct AgentBuilder {
    provider: Option<Box<dyn Provider>>,
    tools: Vec<Box<dyn Tool>>,
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    model: Option<String>,
    max_turns: u32,
    max_tokens: u32,
    temperature: Option<f32>,
    top_p: Option<f32>,
    min_p: Option<f32>,
    thinking_budget: Option<u32>,
    thinking_enabled: Option<bool>,
    reasoning_effort: Option<String>,
    working_dir: Option<PathBuf>,
    permission_policy: Option<Arc<dyn PermissionPolicy>>,
    memory: Option<Arc<dyn Memory>>,
    session_id: Option<String>,
    hooks: Vec<Arc<dyn Hook>>,
    mcp_servers: Vec<McpServerConfig>,
    event_handler: Option<Box<dyn Fn(&AgentEvent) + Send + Sync>>,
    broadcast_capacity: Option<usize>,
    reporters: Vec<Arc<dyn Reporter>>,
    event_filter: Option<Box<dyn Fn(&AgentEvent) -> bool + Send + Sync>>,
    cancel_token: Option<tokio_util::sync::CancellationToken>,
    context_window: Option<u64>,
    auto_compact: bool,
    compact_threshold: f64,
    tool_result_budget: usize,
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self {
            provider: None,
            tools: Vec::new(),
            system_prompt: None,
            append_system_prompt: None,
            model: None,
            max_turns: 10,
            max_tokens: 16384,
            temperature: None,
            top_p: None,
            min_p: None,
            thinking_budget: None,
            thinking_enabled: None,
            reasoning_effort: None,
            working_dir: None,
            permission_policy: None,
            memory: None,
            session_id: None,
            hooks: Vec::new(),
            mcp_servers: Vec::new(),
            event_handler: None,
            broadcast_capacity: None,
            reporters: Vec::new(),
            event_filter: None,
            cancel_token: None,
            context_window: None,
            auto_compact: true,
            compact_threshold: 0.9,
            tool_result_budget: 50_000,
        }
    }
}

impl AgentBuilder {
    pub fn provider(mut self, p: impl Provider + 'static) -> Self {
        self.provider = Some(Box::new(p));
        self
    }

    pub fn tool(mut self, t: impl Tool + 'static) -> Self {
        self.tools.push(Box::new(t));
        self
    }

    pub fn tools(mut self, ts: Vec<Box<dyn Tool>>) -> Self {
        self.tools.extend(ts);
        self
    }

    pub fn system_prompt(mut self, s: impl Into<String>) -> Self {
        self.system_prompt = Some(s.into());
        self
    }

    pub fn append_system_prompt(mut self, s: impl Into<String>) -> Self {
        self.append_system_prompt = Some(s.into());
        self
    }

    pub fn model(mut self, m: impl Into<String>) -> Self {
        self.model = Some(m.into());
        self
    }

    pub fn max_turns(mut self, n: u32) -> Self {
        self.max_turns = n;
        self
    }

    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }

    pub fn temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Nucleus sampling cutoff; omitted keeps the provider default.
    pub fn top_p(mut self, value: f32) -> Self {
        self.top_p = Some(value);
        self
    }

    /// Minimum relative sampling probability, for supporting local servers.
    pub fn min_p(mut self, value: f32) -> Self {
        self.min_p = Some(value);
        self
    }

    /// Turn model-level reasoning on or off, where the provider supports it.
    pub fn thinking(mut self, on: bool) -> Self {
        self.thinking_enabled = Some(on);
        self
    }

    pub fn thinking_budget(mut self, tokens: u32) -> Self {
        self.thinking_budget = Some(tokens);
        self
    }

    pub fn reasoning_effort(mut self, effort: impl Into<String>) -> Self {
        self.reasoning_effort = Some(effort.into());
        self
    }

    pub fn working_dir(mut self, p: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(p.into());
        self
    }

    pub fn permission_policy(mut self, p: impl PermissionPolicy + 'static) -> Self {
        self.permission_policy = Some(Arc::new(p));
        self
    }

    pub fn memory(mut self, m: impl Memory + 'static) -> Self {
        self.memory = Some(Arc::new(m));
        self
    }

    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn hook(mut self, h: impl Hook + 'static) -> Self {
        self.hooks.push(Arc::new(h));
        self
    }

    pub fn mcp_server(mut self, config: McpServerConfig) -> Self {
        self.mcp_servers.push(config);
        self
    }

    pub fn on_event(mut self, f: impl Fn(&AgentEvent) + Send + Sync + 'static) -> Self {
        self.event_handler = Some(Box::new(f));
        self
    }

    pub fn enable_broadcast(mut self, capacity: usize) -> Self {
        self.broadcast_capacity = Some(capacity);
        self
    }

    pub fn reporter(mut self, r: impl Reporter + 'static) -> Self {
        self.reporters.push(Arc::new(r));
        self
    }

    pub fn event_filter(
        mut self,
        f: impl Fn(&AgentEvent) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.event_filter = Some(Box::new(f));
        self
    }

    /// Tokens this model can hold. Providers that publish the figure — a local
    /// server reporting `max_model_len`, say — should pass it: guessing from
    /// the model name is only ever approximate, and wrong by 8x is easy.
    pub fn context_window(mut self, tokens: u64) -> Self {
        self.context_window = Some(tokens);
        self
    }

    pub fn cancel_token(mut self, token: tokio_util::sync::CancellationToken) -> Self {
        self.cancel_token = Some(token);
        self
    }

    pub fn auto_compact(mut self, enabled: bool) -> Self {
        self.auto_compact = enabled;
        self
    }

    pub fn compact_threshold(mut self, threshold: f64) -> Self {
        self.compact_threshold = threshold;
        self
    }

    pub fn tool_result_budget(mut self, chars: usize) -> Self {
        self.tool_result_budget = chars;
        self
    }

    pub fn build(self) -> cersei_types::Result<Agent> {
        let provider = self
            .provider
            .ok_or_else(|| CerseiError::Config("Provider is required".into()))?;

        let working_dir = self
            .working_dir
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let broadcast_tx = self.broadcast_capacity.map(|cap| {
            let (tx, _) = broadcast::channel(cap);
            tx
        });

        // Resolved before the struct literal moves `self.model`.
        if !self.compact_threshold.is_finite() || !(0.0..=1.0).contains(&self.compact_threshold) || self.compact_threshold == 0.0 {
            return Err(CerseiError::Config("compact_threshold must be greater than 0 and at most 1".into()));
        }
        let context_window = self.context_window.filter(|w| *w > 0).unwrap_or_else(|| {
            crate::compact::context_window_for_model(self.model.as_deref().unwrap_or(""))
        });

        Ok(Agent {
            provider,
            tools: self.tools,
            system_prompt: self.system_prompt,
            append_system_prompt: self.append_system_prompt,
            model: self.model,
            max_turns: self.max_turns,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            min_p: self.min_p,
            thinking_budget: self.thinking_budget,
            thinking_enabled: self.thinking_enabled,
            reasoning_effort: self.reasoning_effort,
            working_dir,
            permission_policy: self
                .permission_policy
                .unwrap_or_else(|| Arc::new(AllowAll)),
            memory: self.memory,
            session_id: self.session_id,
            hooks: self.hooks,
            mcp_manager: None, // TODO: connect MCP servers
            event_handler: self.event_handler,
            broadcast_tx,
            reporters: self.reporters,
            event_filter: self.event_filter,
            cost_tracker: Arc::new(CostTracker::new()),
            auto_compact: self.auto_compact,
            context_state: parking_lot::Mutex::new(Default::default()),
            compact_threshold: self.compact_threshold,
            tool_result_budget: self.tool_result_budget,
            messages: Arc::new(parking_lot::Mutex::new(Vec::new())),
            cumulative_usage: Arc::new(parking_lot::Mutex::new(Usage::default())),
            cancel_token: self
                .cancel_token
                .unwrap_or_else(tokio_util::sync::CancellationToken::new),
            context_window,
        })
    }

    /// Build + run in one shot.
    pub async fn run_with(self, prompt: &str) -> cersei_types::Result<AgentOutput> {
        self.build()?.run(prompt).await
    }
}

#[cfg(test)]
mod compaction_regressions {
    use super::*;
    use async_trait::async_trait;
    use cersei_provider::{CompletionRequest, CompletionStream, ProviderCapabilities};
    use std::sync::atomic::{AtomicU8, Ordering};
    use tokio::sync::Notify;

    struct Spy {
        requests: Arc<parking_lot::Mutex<Vec<CompletionRequest>>>,
        mode: Arc<AtomicU8>,
        started: Arc<Notify>,
    }
    #[async_trait]
    impl Provider for Spy {
        fn name(&self) -> &str { "compaction-spy" }
        fn context_window(&self, _: &str) -> u64 { 8192 }
        fn capabilities(&self, _: &str) -> ProviderCapabilities { ProviderCapabilities::default() }
        async fn complete(&self, request: CompletionRequest) -> Result<CompletionStream> {
            let summary = request.system.as_deref().unwrap_or("").contains("Summarize conversation evidence");
            self.requests.lock().push(request);
            let mode = if summary { self.mode.load(Ordering::Relaxed) } else { 0 };
            if mode == 3 { self.started.notify_one(); return std::future::pending().await; }
            if mode == 4 { return Err(CerseiError::Provider("mock summary failure".into())); }
            let text = match mode { 1 => "".into(), 5 => "oversized summary ".repeat(1000), _ => "Summary preserves the task and decisions.".into() };
            let (tx, rx) = mpsc::channel(8);
            for event in [
                StreamEvent::MessageStart { id: "r".into(), model: "mock".into() },
                StreamEvent::ContentBlockStart { index: 0, block_type: "text".into(), id: None, name: None },
                StreamEvent::TextDelta { index: 0, text },
                StreamEvent::ContentBlockStop { index: 0 },
                StreamEvent::MessageDelta { stop_reason: Some(if mode == 2 { StopReason::MaxTokens } else { StopReason::EndTurn }),
                    usage: Some(Usage { input_tokens: if summary { 50 } else { 6000 }, output_tokens: 10, ..Default::default() }) },
                StreamEvent::MessageStop,
            ] { tx.send(event).await.unwrap(); }
            Ok(CompletionStream::new(rx))
        }
    }

    fn fixture(window: u64) -> (Agent, Arc<parking_lot::Mutex<Vec<CompletionRequest>>>, Arc<AtomicU8>, Arc<Notify>) {
        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let mode = Arc::new(AtomicU8::new(0));
        let started = Arc::new(Notify::new());
        let agent = Agent::builder().provider(Spy { requests: requests.clone(), mode: mode.clone(), started: started.clone() })
            .model("mock").context_window(window).max_tokens(512).build().unwrap();
        (agent, requests, mode, started)
    }

    fn history() -> Vec<Message> {
        vec![Message::user("Earlier requirements ".repeat(100)), Message::assistant("Earlier answer ".repeat(100)),
            Message::user("Latest task must stay verbatim"), Message::assistant("Latest answer")]
    }

    #[tokio::test]
    async fn resuming_unanswered_tools_records_unknown_outcome_without_reexecution() {
        let (mut agent, requests, _, _) = fixture(32768);
        let memory = Arc::new(cersei_memory::InMemory::new());
        let history = vec![Message::user("Earlier task"), Message::assistant_blocks(vec![ContentBlock::ToolUse {
            id: "interrupted-call".into(), name: "Bash".into(), input: serde_json::json!({"command":"never reexecute automatically"}),
        }])];
        agent.attach_session(memory, "saved".into(), history, Usage::default());
        assert!(requests.lock().is_empty());
        let messages = agent.messages();
        assert!(matches!(&messages.last().unwrap().content_blocks()[0], ContentBlock::ToolResult { tool_use_id, is_error: Some(true), .. } if tool_use_id == "interrupted-call"));
        agent.persist_context().await.unwrap();
    }

    #[tokio::test]
    async fn auto_compaction_survives_separate_user_prompts_and_counts_usage() {
        let (agent, requests, _, _) = fixture(8192);
        agent.run(&"Initial requirements ".repeat(100)).await.unwrap();
        assert_eq!(requests.lock().len(), 1);
        agent.reply("Continue the task").await.unwrap();
        assert_eq!(requests.lock().len(), 3, "summary must run before second user prompt");
        assert!(agent.messages()[0].get_all_text().contains("context_summary"));
        assert_eq!(agent.context_state.lock().compact.compaction_count, 1);
        assert_eq!(agent.usage().input_tokens, 12050);
    }

    #[tokio::test]
    async fn configured_threshold_is_respected() {
        let (mut agent, requests, _, _) = fixture(8192);
        agent.compact_threshold = 1.0;
        agent.run(&"Initial requirements ".repeat(50)).await.unwrap();
        agent.reply("Continue").await.unwrap();
        assert_eq!(requests.lock().len(), 2, "should not summarize below the configured threshold");
    }

    #[tokio::test]
    async fn manual_compaction_preserves_tail_and_options_and_bounds_every_chunk() {
        let (mut agent, requests, _, _) = fixture(4096);
        agent.set_reasoning_effort(Some("spoon".into()));
        agent.set_thinking_enabled(true);
        let mut messages = vec![Message::user("Start ".repeat(800)),
            Message::assistant_blocks(vec![ContentBlock::ToolUse { id: "t1".into(), name: "Read".into(), input: serde_json::json!({"file_path":"src/important.rs"}) }]),
            Message::user_blocks(vec![ContentBlock::ToolResult { tool_use_id: "t1".into(), content: ToolResultContent::Text(format!("{} MIDDLE-EVIDENCE {}", "α".repeat(3000), "tail ".repeat(800))), is_error: Some(false) }]),
            Message::assistant("Read complete")];
        messages.extend(history().into_iter().skip(2));
        *agent.messages.lock() = messages;
        let result = agent.compact(Some("Preserve tests")).await.unwrap();
        assert!(!result.messages.is_empty());
        assert!(agent.messages()[0].get_all_text().contains("Latest task must stay verbatim"));
        assert_eq!(agent.messages().last().unwrap().get_all_text(), "Latest answer");
        let requests = requests.lock();
        assert!(requests.len() > 1);
        let transcript = requests.iter().map(|r| r.messages[0].get_all_text()).collect::<Vec<_>>().join("\n");
        assert!(transcript.contains("MIDDLE-EVIDENCE"));
        assert!(transcript.contains("src/important.rs"));
        for request in requests.iter() {
            assert!(request.tools.is_empty());
            assert_eq!(request.options.get::<String>("reasoning_effort").as_deref(), Some("spoon"));
            assert_eq!(request.options.get::<bool>("thinking"), Some(true));
            assert!(runner::input_size(&request.messages, &[], request.system.as_deref()) as u64 + u64::from(request.max_tokens) + 1024 <= 4096);
        }
    }

    #[tokio::test]
    async fn empty_truncated_failed_or_oversized_summaries_never_replace_history() {
        for value in [1, 2, 4, 5] {
            let (mut agent, _, mode, _) = fixture(8192);
            *agent.messages.lock() = history();
            let before = serde_json::to_string(&agent.messages()).unwrap();
            mode.store(value, Ordering::Relaxed);
            let result = agent.compact(None).await;
            assert!(result.is_err() || result.unwrap().messages.is_empty());
            assert_eq!(serde_json::to_string(&agent.messages()).unwrap(), before);
            if value != 4 { assert!(agent.usage().input_tokens > 0); }
        }
    }

    #[tokio::test]
    async fn cancel_manual_compaction_preserves_history_and_closes_request() {
        let (mut agent, _, mode, started) = fixture(8192);
        mode.store(3, Ordering::Relaxed);
        *agent.messages.lock() = history();
        let before = serde_json::to_string(&agent.messages()).unwrap();
        let token = agent.cancel_token.clone();
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(agent.compact(None), async { started.notified().await; token.cancel(); })
        }).await.unwrap();
        assert!(matches!(result, Err(CerseiError::Cancelled)));
        assert_eq!(serde_json::to_string(&agent.messages()).unwrap(), before);
    }

    #[tokio::test]
    async fn breaker_persists_across_prompts_and_successful_manual_compact_resets_it() {
        let (mut agent, _, mode, _) = fixture(32768);
        mode.store(4, Ordering::Relaxed);
        *agent.messages.lock() = history();
        for _ in 0..3 {
            agent.context_state.lock().observe(30000, 100000);
            agent.run("Continue").await.unwrap();
        }
        assert!(agent.context_state.lock().compact.disabled);
        mode.store(0, Ordering::Relaxed);
        agent.compact(None).await.unwrap();
        assert!(!agent.context_state.lock().compact.disabled);
    }
}
