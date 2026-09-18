//! Agent runner: the core agentic loop.

use crate::events::{AgentControl, AgentEvent};
use crate::{Agent, AgentOutput, ToolCallRecord};
use cersei_hooks::{HookAction, HookContext, HookEvent};
use cersei_provider::{CompletionRequest, ProviderOptions, StreamAccumulator};
use cersei_tools::permissions::{PermissionDecision, PermissionRequest};
use cersei_tools::{ToolContext, ToolResult};
use cersei_types::*;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

// ─── Tool result budget ──────────────────────────────────────────────────────

/// Bound all tool results, including the newest batch, preserving call/result IDs.
pub fn apply_tool_result_budget(messages: &mut [Message], budget_bytes: usize) {
    let mut remaining = budget_bytes;
    for message in messages.iter_mut().rev() {
        if let MessageContent::Blocks(blocks) = &mut message.content {
            let count = blocks.iter().filter(|b| matches!(b, ContentBlock::ToolResult { .. })).count();
            let allowance = remaining.checked_div(count).unwrap_or(0).min(cersei_tools::output::MODEL_OUTPUT_BYTES);
            for block in blocks {
                if let ContentBlock::ToolResult { content, .. } = block {
                    let text = match content {
                        ToolResultContent::Text(text) => text.clone(),
                        ToolResultContent::Blocks(blocks) => serde_json::to_string(blocks).unwrap_or_default(),
                    };
                    let bounded = cersei_tools::output::excerpt(&text, allowance);
                    remaining = remaining.saturating_sub(bounded.len());
                    *content = ToolResultContent::Text(bounded);
                }
            }
        }
    }
}

/// Conservative byte-based input accounting, including schemas and system prompt.
/// Provider tokenizers differ; use one token per serialized UTF-8 byte plus
/// framing headroom instead of the unsafe prose-only chars/4 heuristic.
fn input_size(messages: &[Message], tools: &[ToolDefinition], system: Option<&str>) -> usize {
    serde_json::to_vec(&(messages, tools, system)).map(|v| v.len()).unwrap_or(usize::MAX)
}

pub(crate) fn fit_request(messages: &mut [Message], tools: &[ToolDefinition], system: Option<&str>,
    window: u64, max_output: u32) -> Result<()> {
    let budget = window.saturating_sub(max_output as u64).saturating_sub(1024) as usize;
    // Measure non-result overhead, then allocate the available space to results.
    let mut overhead = messages.to_vec();
    apply_tool_result_budget(&mut overhead, 0);
    let available = budget.saturating_sub(input_size(&overhead, tools, system));
    let mut allowance = available;
    apply_tool_result_budget(messages, allowance);
    let mut size = input_size(messages, tools, system);
    while size > budget && allowance > 0 {
        allowance /= 2;
        apply_tool_result_budget(messages, allowance);
        size = input_size(messages, tools, system);
    }
    if size > budget {
        return Err(CerseiError::Config(format!(
            "Input exceeds conservative context budget ({size} bytes, {budget} available after reserving {max_output} output tokens and framing). Shorten the prompt, start a new session, reduce max_tokens, or configure the model's actual context_window. No request sent."
        )));
    }
    Ok(())
}

async fn execute_cancellable(tool: &dyn cersei_tools::Tool, input: serde_json::Value,
    ctx: &ToolContext, cancel: &tokio_util::sync::CancellationToken) -> ToolResult {
    tokio::select! {
        _ = cancel.cancelled() => ToolResult::error("Tool execution cancelled"),
        result = tool.execute(input, ctx) => result,
    }
}

/// Run the agent without streaming (blocking until complete).
pub async fn run_agent(agent: &Agent, prompt: &str) -> Result<AgentOutput> {
    let (event_tx, _event_rx) = mpsc::channel(512);
    let (_control_tx, control_rx) = mpsc::channel(64);

    let prompt = prompt.to_string();

    // Run in a background task and collect events
    let result = run_agent_streaming(agent, &prompt, event_tx, control_rx).await;

    match result {
        Ok(output) => {
            agent.emit(AgentEvent::Complete(output.clone()));
            Ok(output)
        }
        Err(e) => {
            agent.emit(AgentEvent::Error(e.to_string()));
            Err(e)
        }
    }
}

/// Core agentic loop with streaming events.
pub async fn run_agent_streaming(
    agent: &Agent,
    prompt: &str,
    event_tx: mpsc::Sender<AgentEvent>,
    _control_rx: mpsc::Receiver<AgentControl>,
) -> Result<AgentOutput> {
    // Load session history
    if let (Some(memory), Some(session_id)) = (&agent.memory, &agent.session_id) {
        let history = memory.load(session_id).await?;
        if !history.is_empty() {
            let count = history.len();
            agent.messages.lock().extend(history);
            let _ = event_tx
                .send(AgentEvent::SessionLoaded {
                    session_id: session_id.clone(),
                    message_count: count,
                })
                .await;
            agent.emit(AgentEvent::SessionLoaded {
                session_id: session_id.clone(),
                message_count: count,
            });
        }
    }

    // Add user prompt
    agent.messages.lock().push(Message::user(prompt));

    let mut tool_calls: Vec<ToolCallRecord> = Vec::new();
    let mut turn: u32 = 0;
    let mut last_stop_reason = StopReason::EndTurn;
    let mut _last_usage = Usage::default();
    // Prompt size the provider reported for the previous turn, and the
    // circuit breaker that stops retrying compaction that keeps failing.
    let mut last_input_tokens: u64 = 0;
    let mut compact_state = crate::compact::AutoCompactState::default();

    // Build tool context
    let tool_ctx = ToolContext {
        working_dir: agent.working_dir.clone(),
        session_id: agent
            .session_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        permissions: Arc::clone(&agent.permission_policy),
        cost_tracker: Arc::clone(&agent.cost_tracker),
        mcp_manager: agent.mcp_manager.clone(),
        extensions: cersei_tools::Extensions::default(),
    };

    // Agentic loop
    loop {
        turn += 1;
        if turn > agent.max_turns {
            break;
        }

        // Check cancellation
        if agent.cancel_token.is_cancelled() {
            return Err(CerseiError::Cancelled);
        }

        let _ = event_tx.send(AgentEvent::TurnStart { turn }).await;
        agent.emit(AgentEvent::TurnStart { turn });

        // Compact before building the request, while there is still room to
        // send one. `last_input_tokens` is the provider's own count for the
        // previous turn — the conversation has only grown since, so it is the
        // best estimate available; before the first turn there is nothing to
        // compact anyway.
        if agent.auto_compact && last_input_tokens > 0 {
            let current = agent.messages.lock().clone();
            let before = current.len();
            let model = agent.model.clone().unwrap_or_default();
            let compacted = tokio::select! {
                biased;
                _ = agent.cancel_token.cancelled() => return Err(CerseiError::Cancelled),
                result = crate::compact::auto_compact_if_needed(
                    agent.provider.as_ref(), &current, &model, last_input_tokens,
                    agent.context_window, &mut compact_state,
                ) => result,
            };
            if let Some(result) = compacted {
                let _ = event_tx
                    .send(AgentEvent::CompactStart {
                        reason: crate::events::CompactReason::ThresholdExceeded,
                        messages_before: before,
                    })
                    .await;
                *agent.messages.lock() = result.messages;
                let _ = event_tx
                    .send(AgentEvent::CompactEnd {
                        messages_after: result.messages_after,
                        tokens_freed: result.tokens_freed_estimate,
                    })
                    .await;
                // The prompt shrank; the old count no longer describes it.
                last_input_tokens = 0;
            }
        }

        // Build completion request
        let mut messages = agent.messages.lock().clone();
        let tool_defs: Vec<ToolDefinition> = agent.tools.iter().map(|t| t.to_definition()).collect();

        apply_tool_result_budget(&mut messages, agent.tool_result_budget);
        fit_request(&mut messages, &tool_defs, agent.system_prompt.as_deref(), agent.context_window, agent.max_tokens)?;

        let model = agent
            .model
            .clone()
            .unwrap_or_else(|| "claude-sonnet-4-6".to_string());

        let mut options = ProviderOptions::default();
        if let Some(value) = agent.top_p { options.set("top_p", value); }
        if let Some(value) = agent.min_p { options.set("min_p", value); }
        if let Some(effort) = &agent.reasoning_effort {
            options.set("reasoning_effort", effort);
        }
        if let Some(budget) = agent.thinking_budget {
            options.set("thinking_budget", budget);
        }
        if let Some(on) = agent.thinking_enabled {
            options.set("thinking", on);
        }

        let request = CompletionRequest {
            model: model.clone(),
            messages: messages.clone(),
            system: agent.system_prompt.clone(),
            tools: tool_defs,
            max_tokens: agent.max_tokens,
            temperature: agent.temperature,
            stop_sequences: Vec::new(),
            options,
        };

        let _ = event_tx
            .send(AgentEvent::ModelRequestStart {
                turn,
                message_count: messages.len(),
                token_estimate: 0,
            })
            .await;

        // Send to provider
        let stream = tokio::select! {
            biased;
            _ = agent.cancel_token.cancelled() => return Err(CerseiError::Cancelled),
            result = agent.provider.complete(request) => result?,
        };
        let mut rx = stream.into_receiver();
        let mut accumulator = StreamAccumulator::new();

        let _ = event_tx
            .send(AgentEvent::ModelResponseStart {
                turn,
                model: model.clone(),
            })
            .await;

        // Process stream events
        loop {
            let event = tokio::select! {
                biased;
                _ = agent.cancel_token.cancelled() => return Err(CerseiError::Cancelled),
                event = rx.recv() => match event {
                    Some(event) => event,
                    None => break,
                },
            };
            match &event {
                StreamEvent::TextDelta { text, .. } => {
                    let _ = event_tx.send(AgentEvent::TextDelta(text.clone())).await;
                    agent.emit(AgentEvent::TextDelta(text.clone()));
                }
                StreamEvent::ThinkingDelta { thinking, .. } => {
                    let _ = event_tx
                        .send(AgentEvent::ThinkingDelta(thinking.clone()))
                        .await;
                    agent.emit(AgentEvent::ThinkingDelta(thinking.clone()));
                }
                StreamEvent::Error { message } => {
                    return Err(CerseiError::Provider(message.clone()));
                }
                _ => {}
            }
            accumulator.process_event(event);
        }

        // Convert accumulated response
        let response = accumulator.into_response()?;
        last_stop_reason = response.stop_reason.clone();
        _last_usage = response.usage.clone();

        last_input_tokens = response.usage.input_tokens;

        // Update cumulative usage
        agent.cumulative_usage.lock().merge(&response.usage);
        agent.cost_tracker.add(&response.usage);

        // Emit cost update
        let cumulative = agent.cumulative_usage.lock().clone();
        let _ = event_tx
            .send(AgentEvent::CostUpdate {
                turn_cost: response.usage.cost_usd.unwrap_or(0.0),
                cumulative_cost: cumulative.cost_usd.unwrap_or(0.0),
                input_tokens: cumulative.input_tokens,
                output_tokens: cumulative.output_tokens,
            })
            .await;
        agent.emit(AgentEvent::CostUpdate {
            turn_cost: response.usage.cost_usd.unwrap_or(0.0),
            cumulative_cost: cumulative.cost_usd.unwrap_or(0.0),
            input_tokens: cumulative.input_tokens,
            output_tokens: cumulative.output_tokens,
        });

        // Add assistant message to history
        agent.messages.lock().push(response.message.clone());

        // Fire PostModelTurn hooks
        let hook_ctx = HookContext {
            event: HookEvent::PostModelTurn,
            tool_name: None,
            tool_input: None,
            tool_result: None,
            tool_is_error: None,
            turn,
            cumulative_cost_usd: cumulative.cost_usd.unwrap_or(0.0),
            message_count: agent.messages.lock().len(),
        };
        let hook_action = cersei_hooks::run_hooks(&agent.hooks, &hook_ctx).await;
        if let HookAction::Block(reason) = hook_action {
            return Err(CerseiError::Provider(format!("Blocked by hook: {}", reason)));
        }

        let _ = event_tx
            .send(AgentEvent::TurnComplete {
                turn,
                stop_reason: response.stop_reason.clone(),
                usage: response.usage.clone(),
            })
            .await;
        agent.emit(AgentEvent::TurnComplete {
            turn,
            stop_reason: response.stop_reason.clone(),
            usage: response.usage.clone(),
        });

        // Handle stop reason
        match &response.stop_reason {
            StopReason::EndTurn => break,
            StopReason::ToolUse => {
                // Process tool calls
                let tool_use_blocks: Vec<(String, String, serde_json::Value)> = response
                    .message
                    .content_blocks()
                    .into_iter()
                    .filter_map(|b| {
                        if let ContentBlock::ToolUse { id, name, input } = b {
                            Some((id, name, input))
                        } else {
                            None
                        }
                    })
                    .collect();

                let mut result_blocks: Vec<ContentBlock> = Vec::new();

                for (tool_id, tool_name, tool_input) in tool_use_blocks {
                    if agent.cancel_token.is_cancelled() {
                        return Err(CerseiError::Cancelled);
                    }
                    let _ = event_tx
                        .send(AgentEvent::ToolStart {
                            name: tool_name.clone(),
                            id: tool_id.clone(),
                            input: tool_input.clone(),
                        })
                        .await;
                    agent.emit(AgentEvent::ToolStart {
                        name: tool_name.clone(),
                        id: tool_id.clone(),
                        input: tool_input.clone(),
                    });

                    let start = Instant::now();

                    // Find the tool
                    let tool = agent.tools.iter().find(|t| t.name() == tool_name);

                    let result = if let Some(tool) = tool {
                        // Check permissions
                        let perm_req = PermissionRequest {
                            tool_name: tool_name.clone(),
                            tool_input: tool_input.clone(),
                            permission_level: tool.permission_level(),
                            description: format!("Execute tool '{}'", tool_name),
                            id: tool_id.clone(),
                        };

                        let decision = agent.permission_policy.check(&perm_req).await;

                        match decision {
                            PermissionDecision::Allow
                            | PermissionDecision::AllowOnce
                            | PermissionDecision::AllowForSession => {
                                // Fire PreToolUse hooks
                                let hook_ctx = HookContext {
                                    event: HookEvent::PreToolUse,
                                    tool_name: Some(tool_name.clone()),
                                    tool_input: Some(tool_input.clone()),
                                    tool_result: None,
                                    tool_is_error: None,
                                    turn,
                                    cumulative_cost_usd: cumulative.cost_usd.unwrap_or(0.0),
                                    message_count: agent.messages.lock().len(),
                                };
                                let hook_action =
                                    cersei_hooks::run_hooks(&agent.hooks, &hook_ctx).await;

                                match hook_action {
                                    HookAction::Block(reason) => {
                                        ToolResult::error(format!("Blocked by hook: {}", reason))
                                    }
                                    HookAction::ModifyInput(new_input) => {
                                        execute_cancellable(tool.as_ref(), new_input, &tool_ctx, &agent.cancel_token).await
                                    }
                                    _ => execute_cancellable(tool.as_ref(), tool_input.clone(), &tool_ctx, &agent.cancel_token).await,
                                }
                            }
                            PermissionDecision::Deny(reason) => {
                                ToolResult::error(format!("Permission denied: {}", reason))
                            }
                        }
                    } else {
                        ToolResult::error(format!("Unknown tool: {}", tool_name))
                    };

                    let mut result = result;
                    // Preserve large non-Bash output for the human viewer before
                    // applying the independent model excerpt budget.
                    if result.content.len() > cersei_tools::output::MODEL_OUTPUT_BYTES
                        && result.metadata.as_ref().and_then(|m| m.get("output_files")).is_none() {
                        if let Ok(mut capture) = cersei_tools::output::Capture::new() {
                            if capture.drain(result.content.as_bytes()).await.is_ok() {
                                let mut metadata = result.metadata.take().unwrap_or_else(|| serde_json::json!({}));
                                if !metadata.is_object() { metadata = serde_json::json!({}); }
                                metadata["output_files"] = serde_json::json!([capture.path]);
                                metadata["output_bytes"] = serde_json::json!(capture.total);
                                metadata["archive_truncated"] = serde_json::json!(capture.total > cersei_tools::output::ARCHIVE_BYTES as u64);
                                result.metadata = Some(metadata);
                            }
                        }
                    }
                    result.content = cersei_tools::output::excerpt(&result.content, cersei_tools::output::MODEL_OUTPUT_BYTES);
                    let duration = start.elapsed();

                    let _ = event_tx
                        .send(AgentEvent::ToolEnd {
                            name: tool_name.clone(),
                            id: tool_id.clone(),
                            result: result.content.clone(),
                            metadata: result.metadata.clone(),
                            is_error: result.is_error,
                            duration,
                        })
                        .await;
                    agent.emit(AgentEvent::ToolEnd {
                        name: tool_name.clone(),
                        id: tool_id.clone(),
                        result: result.content.clone(),
                        metadata: result.metadata.clone(),
                        is_error: result.is_error,
                        duration,
                    });

                    tool_calls.push(ToolCallRecord {
                        name: tool_name.clone(),
                        id: tool_id.clone(),
                        input: tool_input,
                        result: result.content.clone(),
                        is_error: result.is_error,
                        duration,
                    });

                    result_blocks.push(ContentBlock::ToolResult {
                        tool_use_id: tool_id,
                        content: ToolResultContent::Text(result.content),
                        is_error: Some(result.is_error),
                    });
                }

                // Add tool results as user message
                agent
                    .messages
                    .lock()
                    .push(Message::user_blocks(result_blocks));
            }
            StopReason::MaxTokens => {
                // Inject continuation message
                agent
                    .messages
                    .lock()
                    .push(Message::user("Continue from where you left off."));
            }
            _ => break,
        }
    }

    // Persist session
    if let (Some(memory), Some(session_id)) = (&agent.memory, &agent.session_id) {
        let messages = agent.messages.lock().clone();
        memory.store(session_id, &messages).await?;
        let _ = event_tx
            .send(AgentEvent::SessionSaved {
                session_id: session_id.clone(),
            })
            .await;
        agent.emit(AgentEvent::SessionSaved {
            session_id: session_id.clone(),
        });
    }

    // Build output
    let last_message = agent
        .messages
        .lock()
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant)
        .cloned()
        .unwrap_or_else(|| Message::assistant(""));

    let output = AgentOutput {
        message: last_message,
        usage: agent.cumulative_usage.lock().clone(),
        stop_reason: last_stop_reason,
        turns: turn,
        tool_calls,
    };

    // Notify reporters
    for reporter in &agent.reporters {
        reporter.on_complete(&output).await;
    }

    Ok(output)
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use cersei_provider::{CompletionStream, Provider, ProviderCapabilities};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;
    use tokio::time::{timeout, Duration};
    use tokio_util::sync::CancellationToken;

    struct SilentProvider {
        wait_before_stream: bool,
        calls: AtomicUsize,
        started: Arc<Notify>,
        disconnected: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl Provider for SilentProvider {
        fn name(&self) -> &str { "silent-test" }
        fn context_window(&self, _: &str) -> u64 { 32768 }
        fn capabilities(&self, _: &str) -> ProviderCapabilities { Default::default() }

        async fn complete(&self, request: CompletionRequest) -> Result<CompletionStream> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.started.notify_one();
                if self.wait_before_stream {
                    return std::future::pending().await;
                }
                let (tx, rx) = mpsc::channel(1);
                let disconnected = self.disconnected.clone();
                tokio::spawn(async move {
                    tx.closed().await;
                    disconnected.notify_one();
                });
                return Ok(CompletionStream::new(rx));
            }
            // The next turn retains the original prompt and adds the correction.
            assert_eq!(request.messages.len(), 2);
            assert_eq!(request.messages[0].get_all_text(), "Original request");
            assert_eq!(request.messages[1].get_all_text(), "Correction: use another approach");
            let (tx, rx) = mpsc::channel(8);
            for event in [
                StreamEvent::MessageStart { id: "test".into(), model: "test".into() },
                StreamEvent::ContentBlockStart { index: 0, block_type: "text".into(), id: None, name: None },
                StreamEvent::TextDelta { index: 0, text: "Corrected answer".into() },
                StreamEvent::ContentBlockStop { index: 0 },
                StreamEvent::MessageDelta { stop_reason: Some(StopReason::EndTurn), usage: None },
                StreamEvent::MessageStop,
            ] {
                tx.send(event).await.unwrap();
            }
            Ok(CompletionStream::new(rx))
        }
    }

    #[tokio::test]
    async fn cancels_silent_provider_and_accepts_correction_on_next_turn() {
        for wait_before_stream in [true, false] {
            let started = Arc::new(Notify::new());
            let disconnected = Arc::new(Notify::new());
            let cancel = CancellationToken::new();
            let mut agent = Agent::builder().provider(SilentProvider {
                wait_before_stream, calls: AtomicUsize::new(0),
                started: started.clone(), disconnected: disconnected.clone(),
            }).cancel_token(cancel.clone()).build().unwrap();
            let cancel_when_started = async {
                started.notified().await;
                cancel.cancel();
            };
            let (result, ()) = timeout(Duration::from_secs(2), async {
                tokio::join!(agent.run("Original request"), cancel_when_started)
            }).await.expect("Cancellation must not wait for a provider event");
            assert!(matches!(result, Err(CerseiError::Cancelled)));
            if !wait_before_stream {
                timeout(Duration::from_secs(2), disconnected.notified()).await
                    .expect("Cancelled turn must drop the provider receiver");
            }
            agent.set_cancel_token(CancellationToken::new());
            timeout(Duration::from_secs(2), agent.run("Correction: use another approach"))
                .await.unwrap().unwrap();
        }
    }
}

#[cfg(test)]
mod output_budget_tests {
    use super::*;
    fn results(n: usize, text: &str) -> Vec<Message> {
        vec![Message::user_blocks((0..n).map(|i| ContentBlock::ToolResult {
            tool_use_id: i.to_string(), content: ToolResultContent::Text(text.into()), is_error: Some(false),
        }).collect())]
    }
    #[test]
    fn newest_batch_shares_budget_and_keeps_ids() {
        let mut messages = results(5, &"🦀".repeat(10000));
        apply_tool_result_budget(&mut messages, 1000);
        let blocks = messages[0].content_blocks();
        let mut bytes = 0;
        for (i, block) in blocks.iter().enumerate() {
            if let ContentBlock::ToolResult { tool_use_id, content: ToolResultContent::Text(text), .. } = block {
                assert_eq!(*tool_use_id, i.to_string()); bytes += text.len();
            } else { panic!("lost result"); }
        }
        assert!(bytes <= 1000);
    }
    #[test]
    fn request_accounts_for_escaping_and_output_reserve() {
        let mut messages = results(4, &"\u{0001}".repeat(10000));
        fit_request(&mut messages, &[], Some("system"), 8192, 2048).unwrap();
        assert!(input_size(&messages, &[], Some("system")) <= 8192 - 2048 - 1024);
    }
    #[test]
    fn rejects_large_user_input_or_schemas_without_dropping_them() {
        let mut messages = vec![Message::user("x".repeat(10000))];
        assert!(fit_request(&mut messages, &[], None, 8192, 2048).is_err());
        assert_eq!(messages[0].get_all_text().len(), 10000);
        let tools = vec![ToolDefinition { name: "test".into(), description: "x".repeat(10000), input_schema: serde_json::json!({}) }];
        assert!(fit_request(&mut [], &tools, None, 8192, 2048).is_err());
        assert!(fit_request(&mut [], &[], None, 1024, 2048).is_err());
    }
}
