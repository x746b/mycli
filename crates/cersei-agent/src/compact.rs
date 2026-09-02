//! Auto-compact: context window management for long conversations.
//!
//! When the conversation approaches the context window limit, older messages
//! are summarized to free space while preserving essential context.

use cersei_provider::Provider;
use cersei_types::*;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Fraction of context window that triggers auto-compact.
pub const AUTOCOMPACT_TRIGGER_FRACTION: f64 = 0.90;
/// Number of recent messages to always preserve (never compacted).
pub const KEEP_RECENT_MESSAGES: usize = 10;
/// Max consecutive failures before disabling auto-compact.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Warning threshold (80% of context window).
pub const WARNING_PCT: f64 = 0.80;
/// Critical threshold (95% of context window).
pub const CRITICAL_PCT: f64 = 0.95;

// ─── Types ───────────────────────────────────────────────────────────────────

/// Session-level compaction tracking.
#[derive(Debug, Clone, Default)]
pub struct AutoCompactState {
    pub compaction_count: u32,
    pub consecutive_failures: u32,
    pub disabled: bool,
}

impl AutoCompactState {
    pub fn on_success(&mut self) {
        self.compaction_count += 1;
        self.consecutive_failures = 0;
    }

    pub fn on_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            self.disabled = true;
        }
    }
}

/// Context window fullness level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenWarningState {
    /// Below 80% — no action needed.
    Ok,
    /// 80-95% — warn user, consider compacting.
    Warning,
    /// Above 95% — critical, must compact or will fail.
    Critical,
}

/// A semantically coherent group of messages for summarization.
#[derive(Debug, Clone)]
pub struct MessageGroup {
    pub messages: Vec<Message>,
    pub topic_hint: Option<String>,
    pub token_estimate: usize,
}

/// Result of a compaction operation.
#[derive(Debug, Clone)]
pub struct CompactResult {
    pub messages_before: usize,
    pub messages_after: usize,
    pub tokens_freed_estimate: u64,
    pub summary: String,
    /// The conversation to continue with: the summary, then the messages that
    /// were kept. Empty when nothing was compacted.
    pub messages: Vec<Message>,
}

/// What triggered the compaction.
#[derive(Debug, Clone, Copy)]
pub enum CompactTrigger {
    AutoThreshold,
    Manual,
    ContextOverflow,
}

// ─── Token estimation ────────────────────────────────────────────────────────

/// Rough token estimate for a message (~4 chars per token).
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64) / 4
}

/// Estimate tokens for a list of messages.
pub fn estimate_messages_tokens(messages: &[Message]) -> u64 {
    messages.iter().map(|m| estimate_tokens(&m.get_all_text())).sum()
}

/// Guess a context window from the model name.
///
/// A last resort: model ids arrive in whatever case the provider uses —
/// `Qwen3.6-35B-A3B-8bit` from a local server, `gpt-4o` from OpenAI — so the
/// comparison is case-insensitive. Matching case-sensitively silently dropped
/// every capitalised name to the default. Prefer a window the provider states:
/// see `AgentBuilder::context_window`.
pub fn context_window_for_model(model: &str) -> u64 {
    let model = model.to_ascii_lowercase();
    match model.as_str() {
        // Anthropic
        m if m.contains("opus") => 200_000,
        m if m.contains("sonnet") => 200_000,
        m if m.contains("haiku") => 200_000,
        // OpenAI
        m if m.contains("gpt-4o") => 128_000,
        m if m.contains("gpt-4-turbo") => 128_000,
        m if m.contains("gpt-4") => 8_192,
        m if m.contains("gpt-3.5") => 16_385,
        m if m.contains("o1") || m.contains("o3") || m.contains("o4") => 200_000,
        // Google
        m if m.contains("gemini") => 1_000_000,
        // Kimi / Moonshot
        m if m.contains("kimi") || m.contains("moonshot") => 128_000,
        // DeepSeek
        m if m.contains("deepseek") => 64_000,
        // Local / small
        m if m.contains("llama") => 8_192,
        m if m.contains("qwen") => 32_768,
        m if m.contains("mistral") => 32_768,
        _ => 32_768, // sensible default for local models
    }
}

// ─── Warning state ───────────────────────────────────────────────────────────

/// Calculate the token warning state given current usage.
pub fn calculate_token_warning_state(tokens_used: u64, context_limit: u64) -> TokenWarningState {
    if context_limit == 0 {
        return TokenWarningState::Ok;
    }
    let pct = tokens_used as f64 / context_limit as f64;
    if pct >= CRITICAL_PCT {
        TokenWarningState::Critical
    } else if pct >= WARNING_PCT {
        TokenWarningState::Warning
    } else {
        TokenWarningState::Ok
    }
}

// ─── Should compact ──────────────────────────────────────────────────────────

/// Check if compaction should trigger.
pub fn should_compact(tokens_used: u64, context_limit: u64) -> bool {
    if context_limit == 0 {
        return false;
    }
    (tokens_used as f64 / context_limit as f64) >= AUTOCOMPACT_TRIGGER_FRACTION
}

/// Check if auto-compact should run (considering state/circuit breaker).
pub fn should_auto_compact(tokens_used: u64, context_limit: u64, state: &AutoCompactState) -> bool {
    if state.disabled {
        return false;
    }
    should_compact(tokens_used, context_limit)
}

/// Check if context collapse is needed (emergency, >98%).
pub fn should_context_collapse(tokens_used: u64, context_limit: u64) -> bool {
    if context_limit == 0 {
        return false;
    }
    (tokens_used as f64 / context_limit as f64) >= 0.98
}

// ─── Message grouping ────────────────────────────────────────────────────────

/// Extract a topic hint from messages (first file path or tool name).
fn extract_topic_hint(messages: &[Message]) -> Option<String> {
    for msg in messages {
        for block in msg.content_blocks() {
            match &block {
                ContentBlock::ToolUse { name, input, .. } => {
                    if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                        return Some(path.to_string());
                    }
                    return Some(name.clone());
                }
                _ => {}
            }
        }
    }
    None
}

/// Group messages into semantically coherent chunks at API-round boundaries.
/// Each group = one assistant response + its tool results.
pub fn group_messages_for_compact(messages: &[Message]) -> Vec<MessageGroup> {
    let mut groups: Vec<MessageGroup> = Vec::new();
    let mut current: Vec<Message> = Vec::new();

    for msg in messages {
        current.push(msg.clone());
        // End group at assistant messages that don't have tool use (end of a "round")
        if msg.role == Role::Assistant && !msg.has_tool_use() {
            let token_est = current.iter().map(|m| m.get_all_text().len() / 4).sum();
            let hint = extract_topic_hint(&current);
            groups.push(MessageGroup {
                messages: std::mem::take(&mut current),
                topic_hint: hint,
                token_estimate: token_est,
            });
        }
    }
    // Leftover messages
    if !current.is_empty() {
        let token_est = current.iter().map(|m| m.get_all_text().len() / 4).sum();
        let hint = extract_topic_hint(&current);
        groups.push(MessageGroup {
            messages: current,
            topic_hint: hint,
            token_estimate: token_est,
        });
    }
    groups
}

// ─── Snip compact (simple truncation) ────────────────────────────────────────

/// Remove oldest messages, keeping only the newest `keep_n`.
/// Returns (remaining messages, estimated tokens freed).
pub fn snip_compact(messages: Vec<Message>, keep_n: usize) -> (Vec<Message>, u64) {
    if messages.len() <= keep_n {
        return (messages, 0);
    }
    let removed = &messages[..messages.len() - keep_n];
    let freed = estimate_messages_tokens(removed);
    let kept = messages[messages.len() - keep_n..].to_vec();
    (kept, freed)
}

/// Calculate how many messages to keep given a token budget.
pub fn calculate_messages_to_keep_index(messages: &[Message], token_budget: u64) -> usize {
    let mut total: u64 = 0;
    for (i, msg) in messages.iter().rev().enumerate() {
        total += estimate_tokens(&msg.get_all_text());
        if total > token_budget {
            return messages.len() - i;
        }
    }
    0 // keep all
}

// ─── Collapse strategies ─────────────────────────────────────────────────────

/// Collapse repeated file read results: if the same file is read multiple
/// times, only keep the latest result.
pub fn collapse_read_tool_results(messages: Vec<Message>) -> Vec<Message> {
    let mut seen_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut result: Vec<Message> = Vec::new();

    // Process in reverse to keep latest reads
    for msg in messages.into_iter().rev() {
        let dominated = match &msg.content {
            MessageContent::Blocks(blocks) => {
                blocks.iter().all(|b| {
                    if let ContentBlock::ToolResult { tool_use_id, content, .. } = b {
                        // Check if this is a file read result we've already seen
                        if let ToolResultContent::Text(text) = content {
                            if text.contains('\t') {
                                // Line-numbered output = file read
                                let key = tool_use_id.clone();
                                if seen_files.contains(&key) {
                                    return true; // dominated, skip
                                }
                                seen_files.insert(key);
                            }
                        }
                        false
                    } else {
                        false
                    }
                })
            }
            _ => false,
        };

        if !dominated {
            result.push(msg);
        }
    }

    result.reverse();
    result
}

// ─── Compact prompt ──────────────────────────────────────────────────────────

/// Build the compaction prompt for the LLM.
pub fn get_compact_prompt(custom_instructions: Option<&str>) -> String {
    let mut prompt = String::from(
        "Summarize the conversation so far. Focus on:\n\
        1. Key decisions made and their rationale\n\
        2. Files that were read, created, or modified (with paths)\n\
        3. Tool results that are still relevant\n\
        4. Outstanding tasks or next steps\n\
        5. Any errors encountered and how they were resolved\n\n\
        Be concise but preserve all actionable information. \
        Use bullet points. Include file paths verbatim.",
    );
    if let Some(instructions) = custom_instructions {
        prompt.push_str("\n\nAdditional context: ");
        prompt.push_str(instructions);
    }
    prompt
}

/// Format raw compact output into a summary message.
pub fn format_compact_summary(raw: &str) -> String {
    format!(
        "<context_summary>\n\
        The following is a summary of the conversation so far:\n\n\
        {}\n\
        </context_summary>",
        raw.trim()
    )
}

// ─── Full compaction (requires provider call) ────────────────────────────────

/// Compact the conversation by summarizing older messages.
///
/// 1. Split messages into "old" (to compact) and "recent" (to keep)
/// 2. Group old messages by topic
/// 3. Send to provider for summarization
/// 4. Replace old messages with summary
pub async fn compact_conversation(
    provider: &dyn Provider,
    messages: &[Message],
    model: &str,
    keep_recent: usize,
    custom_instructions: Option<&str>,
) -> Result<CompactResult> {
    let messages_before = messages.len();

    if messages.len() <= keep_recent {
        return Ok(CompactResult {
            messages_before,
            messages_after: messages_before,
            tokens_freed_estimate: 0,
            summary: String::new(),
            messages: Vec::new(),
        });
    }

    let Some(split_idx) = safe_split_point(messages, messages.len() - keep_recent) else {
        // Every candidate boundary would orphan a tool result. Leaving the
        // conversation alone is better than sending one a provider rejects.
        return Ok(CompactResult {
            messages_before,
            messages_after: messages_before,
            tokens_freed_estimate: 0,
            summary: String::new(),
            messages: Vec::new(),
        });
    };
    let old_messages = &messages[..split_idx];
    let recent_messages = &messages[split_idx..];

    // Build compaction request
    let old_text: String = old_messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::System => "System",
            };
            format!("{}: {}", role, m.get_all_text())
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let compact_prompt = get_compact_prompt(custom_instructions);
    let request = cersei_provider::CompletionRequest {
        model: model.to_string(),
        messages: vec![
            Message::user(format!(
                "Here is the conversation history to summarize:\n\n{}\n\n{}",
                old_text, compact_prompt
            )),
        ],
        system: Some("You are a conversation summarizer. Be concise and preserve all actionable information.".into()),
        tools: Vec::new(),
        max_tokens: 4096,
        temperature: Some(0.0),
        stop_sequences: Vec::new(),
        options: cersei_provider::ProviderOptions::default(),
    };

    let response = provider.complete_blocking(request).await?;
    let summary_text = response.message.get_all_text();
    let formatted_summary = format_compact_summary(&summary_text);

    let tokens_freed = estimate_messages_tokens(old_messages);

    let mut compacted = Vec::with_capacity(1 + recent_messages.len());
    compacted.push(Message::user(formatted_summary.clone()));
    compacted.extend_from_slice(recent_messages);

    Ok(CompactResult {
        messages_before,
        messages_after: compacted.len(),
        tokens_freed_estimate: tokens_freed,
        summary: formatted_summary,
        messages: compacted,
    })
}

/// First index at or after `from` that is safe to resume a conversation at.
///
/// A plain tail split can cut a tool round in half, leaving a `tool_result`
/// whose `tool_use` has just been summarised away — providers reject that
/// outright. Only a user message carrying no tool results is a clean start.
/// Returns `None` when no such boundary exists, which means this conversation
/// cannot be compacted safely right now.
fn safe_split_point(messages: &[Message], from: usize) -> Option<usize> {
    (from..messages.len()).find(|&i| is_clean_boundary(&messages[i]))
}

fn is_clean_boundary(message: &Message) -> bool {
    message.role == Role::User
        && !message
            .content_blocks()
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
}

/// Check and run auto-compact if needed. Returns None if no compaction needed.
pub async fn auto_compact_if_needed(
    provider: &dyn Provider,
    messages: &[Message],
    model: &str,
    tokens_used: u64,
    context_limit: u64,
    state: &mut AutoCompactState,
) -> Option<CompactResult> {
    if !should_auto_compact(tokens_used, context_limit, state) {
        return None;
    }

    match compact_conversation(provider, messages, model, KEEP_RECENT_MESSAGES, None).await {
        // A run that could not find a safe boundary changed nothing; treat it
        // as "not needed" rather than a success, so the next turn tries again.
        Ok(result) if result.messages.is_empty() => None,
        Ok(result) => {
            state.on_success();
            Some(result)
        }
        Err(_) => {
            state.on_failure();
            None
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(n: usize) -> Vec<Message> {
        (0..n)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(format!("User message {}", i))
                } else {
                    Message::assistant(format!("Assistant response {} with some longer text to simulate real content that takes up tokens in the context window.", i))
                }
            })
            .collect()
    }

    #[test]
    fn test_token_warning_ok() {
        assert_eq!(
            calculate_token_warning_state(50_000, 200_000),
            TokenWarningState::Ok
        );
    }

    #[test]
    fn test_token_warning_warning() {
        assert_eq!(
            calculate_token_warning_state(170_000, 200_000),
            TokenWarningState::Warning
        );
    }

    #[test]
    fn test_token_warning_critical() {
        assert_eq!(
            calculate_token_warning_state(196_000, 200_000),
            TokenWarningState::Critical
        );
    }

    #[test]
    fn test_should_compact() {
        assert!(!should_compact(100_000, 200_000)); // 50%
        assert!(!should_compact(170_000, 200_000)); // 85%
        assert!(should_compact(185_000, 200_000)); // 92.5%
        assert!(should_compact(195_000, 200_000)); // 97.5%
    }

    #[test]
    fn test_should_auto_compact_disabled() {
        let state = AutoCompactState {
            disabled: true,
            ..Default::default()
        };
        assert!(!should_auto_compact(195_000, 200_000, &state));
    }

    #[test]
    fn test_circuit_breaker() {
        let mut state = AutoCompactState::default();
        state.on_failure();
        state.on_failure();
        assert!(!state.disabled);
        state.on_failure(); // 3rd failure
        assert!(state.disabled);
    }

    #[test]
    fn test_snip_compact() {
        let messages = make_messages(20);
        let (kept, freed) = snip_compact(messages, 10);
        assert_eq!(kept.len(), 10);
        assert!(freed > 0);
    }

    #[test]
    fn test_snip_compact_already_small() {
        let messages = make_messages(5);
        let (kept, freed) = snip_compact(messages, 10);
        assert_eq!(kept.len(), 5);
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_group_messages() {
        let mut messages = Vec::new();
        messages.push(Message::user("Read file A"));
        messages.push(Message::assistant("Contents of A"));
        messages.push(Message::user("Now edit B"));
        messages.push(Message::assistant("Edited B"));

        let groups = group_messages_for_compact(&messages);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello world"), 2); // 11 chars / 4
        assert_eq!(estimate_tokens(""), 0);
        assert!(estimate_tokens(&"x".repeat(1000)) > 200);
    }

    #[test]
    fn test_context_window_for_model() {
        assert_eq!(context_window_for_model("claude-sonnet-4-6"), 200_000);
        assert_eq!(context_window_for_model("gpt-4o"), 128_000);
        assert_eq!(context_window_for_model("gpt-4"), 8_192);
        // Local servers name models however they like; a capitalised id used
        // to miss every arm and fall through to the default.
        assert_eq!(context_window_for_model("Qwen3.6-35B-A3B-8bit"), 32_768);
        assert_eq!(context_window_for_model("Llama-3.3-70B"), 8_192);
        assert_eq!(context_window_for_model("GPT-4o-mini"), 128_000);
    }

    #[test]
    fn test_compact_prompt_with_instructions() {
        let prompt = get_compact_prompt(Some("Focus on API changes"));
        assert!(prompt.contains("Focus on API changes"));
        assert!(prompt.contains("Summarize"));
    }

    #[test]
    fn test_format_compact_summary() {
        let summary = format_compact_summary("- Did X\n- Did Y");
        assert!(summary.contains("<context_summary>"));
        assert!(summary.contains("- Did X"));
    }

    #[test]
    fn test_calculate_messages_to_keep_index() {
        let messages = make_messages(20);
        let idx = calculate_messages_to_keep_index(&messages, 100);
        assert!(idx > 0);
        assert!(idx < 20);
    }

    #[test]
    fn test_messages_to_keep_all_fit() {
        let messages = make_messages(3);
        let idx = calculate_messages_to_keep_index(&messages, 100_000);
        assert_eq!(idx, 0); // keep all
    }
}

#[cfg(test)]
mod autocompact_tests {
    use super::*;
    use async_trait::async_trait;
    use cersei_provider::{
        CompletionRequest, CompletionStream, Provider, ProviderCapabilities,
    };
    use tokio::sync::mpsc;

    /// Returns a fixed summary, so a compaction round-trip can be exercised
    /// without a model.
    struct SummarizerProvider;

    #[async_trait]
    impl Provider for SummarizerProvider {
        fn name(&self) -> &str {
            "summarizer"
        }
        fn context_window(&self, _: &str) -> u64 {
            4096
        }
        fn capabilities(&self, _: &str) -> ProviderCapabilities {
            ProviderCapabilities { streaming: true, ..Default::default() }
        }
        async fn complete(&self, _req: CompletionRequest) -> Result<CompletionStream> {
            let (tx, rx) = mpsc::channel(16);
            tokio::spawn(async move {
                let _ = tx.send(StreamEvent::MessageStart { id: "1".into(), model: "s".into() }).await;
                let _ = tx.send(StreamEvent::ContentBlockStart {
                    index: 0, block_type: "text".into(), id: None, name: None,
                }).await;
                let _ = tx.send(StreamEvent::TextDelta {
                    index: 0, text: "earlier work summarised".into(),
                }).await;
                let _ = tx.send(StreamEvent::ContentBlockStop { index: 0 }).await;
                let _ = tx.send(StreamEvent::MessageDelta {
                    stop_reason: Some(StopReason::EndTurn),
                    usage: Some(Usage { input_tokens: 10, output_tokens: 5, ..Default::default() }),
                }).await;
                let _ = tx.send(StreamEvent::MessageStop).await;
            });
            Ok(CompletionStream::new(rx))
        }
    }

    fn plain_conversation(pairs: usize) -> Vec<Message> {
        let mut messages = Vec::new();
        for i in 0..pairs {
            messages.push(Message::user(format!("question {i}")));
            messages.push(Message::assistant(format!("answer {i}")));
        }
        messages
    }

    #[tokio::test]
    async fn compaction_replaces_the_old_turns_with_a_summary() {
        let messages = plain_conversation(20);
        let result = compact_conversation(&SummarizerProvider, &messages, "m", 10, None)
            .await
            .unwrap();

        assert_eq!(result.messages_before, 40);
        assert!(result.messages.len() < messages.len(), "{:?}", result.messages.len());
        assert_eq!(result.messages_after, result.messages.len());
        // The summary leads, and the tail is preserved verbatim.
        assert!(result.messages[0].get_all_text().contains("earlier work summarised"));
        assert_eq!(
            result.messages.last().unwrap().get_all_text(),
            messages.last().unwrap().get_all_text()
        );
    }

    /// The kept tail must not begin with a tool result whose tool call was
    /// just summarised away — providers reject that outright.
    #[tokio::test]
    async fn compaction_never_orphans_a_tool_result() {
        const KEEP: usize = 10;

        // Built so the naive boundary (len - KEEP) lands exactly on the tool
        // result, which is the case the safe split point exists to handle.
        let mut messages = plain_conversation(8); // 16, indices 0..15
        messages.push(Message::assistant_blocks(vec![ContentBlock::ToolUse {
            id: "call_1".into(),
            name: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
        }])); // 16
        messages.push(Message::user_blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: ToolResultContent::Text("out".into()),
            is_error: None,
        }])); // 17
        messages.extend(plain_conversation(4)); // 18..25
        messages.push(Message::user("last")); // 26

        let naive = messages.len() - KEEP;
        assert_eq!(naive, 17, "test no longer exercises the boundary it targets");
        assert!(
            !is_clean_boundary(&messages[naive]),
            "naive split must be the unsafe one for this test to mean anything"
        );

        let result = compact_conversation(&SummarizerProvider, &messages, "m", KEEP, None)
            .await
            .unwrap();

        for message in result.messages.iter().skip(1) {
            for block in message.content_blocks() {
                if let ContentBlock::ToolResult { tool_use_id, .. } = &block {
                    let call_kept = result.messages.iter().any(|m| {
                        m.content_blocks().iter().any(|b| {
                            matches!(b, ContentBlock::ToolUse { id, .. } if id == tool_use_id)
                        })
                    });
                    assert!(call_kept, "orphaned tool_result {tool_use_id}");
                }
            }
        }
    }

    #[tokio::test]
    async fn auto_compact_respects_the_threshold_and_the_limit_it_is_given() {
        let messages = plain_conversation(20);
        let mut state = AutoCompactState::default();

        // Half full: nothing to do.
        assert!(auto_compact_if_needed(
            &SummarizerProvider, &messages, "m", 50_000, 100_000, &mut state
        ).await.is_none());

        // The same token count against a window eight times smaller — the
        // difference between a stated window and one guessed from the name.
        assert!(auto_compact_if_needed(
            &SummarizerProvider, &messages, "m", 50_000, 12_500, &mut state
        ).await.is_some());
        assert_eq!(state.compaction_count, 1);
    }

    #[tokio::test]
    async fn a_disabled_breaker_stops_compaction() {
        let messages = plain_conversation(20);
        let mut state = AutoCompactState { disabled: true, ..Default::default() };
        assert!(auto_compact_if_needed(
            &SummarizerProvider, &messages, "m", 99_000, 100_000, &mut state
        ).await.is_none());
    }
}
