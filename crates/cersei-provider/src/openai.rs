//! OpenAI-compatible provider (works with OpenAI, Azure, Ollama, oMLX, etc.)

use crate::*;
use cersei_types::*;
use futures::StreamExt;
use std::collections::HashMap;
use tokio::sync::mpsc;

const OPENAI_API_BASE: &str = "https://api.openai.com/v1";

pub struct OpenAi {
    auth: Auth,
    base_url: String,
    default_model: String,
    client: reqwest::Client,
}

impl OpenAi {
    pub fn new(auth: Auth) -> Self {
        Self {
            auth,
            base_url: OPENAI_API_BASE.to_string(),
            default_model: "gpt-4o".to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| CerseiError::Auth("OPENAI_API_KEY not set".into()))?;
        Ok(Self::new(Auth::ApiKey(key)))
    }

    pub fn builder() -> OpenAiBuilder {
        OpenAiBuilder::default()
    }
}

/// Tracks in-progress tool calls being streamed from the API.
struct ToolCallAccumulator {
    /// tool call index → (id, function name, accumulated arguments)
    calls: HashMap<u32, (String, String, String)>,
    /// Set of tool call indices for which we have already emitted ContentBlockStart
    started: std::collections::HashSet<u32>,
}

impl ToolCallAccumulator {
    fn new() -> Self {
        Self {
            calls: HashMap::new(),
            started: std::collections::HashSet::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }
}

/// Convert cersei Message history into OpenAI API messages, preserving tool
/// use / tool result round-trip structure.
fn convert_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut api_messages: Vec<serde_json::Value> = Vec::new();

    for msg in messages {
        match msg.role {
            Role::User => {
                // Check if this is a user message carrying tool results
                if let MessageContent::Blocks(blocks) = &msg.content {
                    let mut tool_results: Vec<serde_json::Value> = Vec::new();
                    let mut text_parts: Vec<String> = Vec::new();

                    for block in blocks {
                        match block {
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                            } => {
                                let content_str = match content {
                                    ToolResultContent::Text(t) => t.clone(),
                                    ToolResultContent::Blocks(bs) => bs
                                        .iter()
                                        .filter_map(|b| {
                                            if let ContentBlock::Text { text } = b {
                                                Some(text.as_str())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join(""),
                                };
                                // Prepend error marker if flagged
                                let final_content = if is_error.unwrap_or(false) {
                                    format!("Error: {}", content_str)
                                } else {
                                    content_str
                                };
                                tool_results.push(serde_json::json!({
                                    "role": "tool",
                                    "tool_call_id": tool_use_id,
                                    "content": final_content,
                                }));
                            }
                            ContentBlock::Text { text } => {
                                text_parts.push(text.clone());
                            }
                            _ => {}
                        }
                    }

                    // Emit tool results as separate "tool" role messages
                    for tr in tool_results {
                        api_messages.push(tr);
                    }
                    // Emit any remaining text as a user message
                    if !text_parts.is_empty() {
                        api_messages.push(serde_json::json!({
                            "role": "user",
                            "content": text_parts.join(""),
                        }));
                    }
                } else {
                    api_messages.push(serde_json::json!({
                        "role": "user",
                        "content": msg.get_all_text(),
                    }));
                }
            }
            Role::Assistant => {
                // Check for tool use blocks
                if let MessageContent::Blocks(blocks) = &msg.content {
                    let mut text_parts: Vec<String> = Vec::new();
                    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
                    let mut thinking_parts: Vec<String> = Vec::new();

                    for block in blocks {
                        match block {
                            ContentBlock::Text { text } => {
                                text_parts.push(text.clone());
                            }
                            ContentBlock::ToolUse { id, name, input } => {
                                tool_calls.push(serde_json::json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": input.to_string(),
                                    }
                                }));
                            }
                            ContentBlock::Thinking { thinking, .. } => {
                                thinking_parts.push(thinking.clone());
                            }
                            _ => {}
                        }
                    }

                    let mut assistant_msg = serde_json::json!({
                        "role": "assistant",
                    });
                    let combined_text = text_parts.join("");
                    if !combined_text.is_empty() {
                        assistant_msg["content"] = serde_json::json!(combined_text);
                    }
                    if !tool_calls.is_empty() {
                        assistant_msg["tool_calls"] =
                            serde_json::Value::Array(tool_calls);
                    }
                    // Kimi/DeepSeek thinking mode: echo reasoning_content back
                    let combined_thinking = thinking_parts.join("");
                    if !combined_thinking.is_empty() {
                        assistant_msg["reasoning_content"] =
                            serde_json::json!(combined_thinking);
                    }
                    // Ensure at least content is present (some APIs require it)
                    if assistant_msg.get("content").is_none()
                        && assistant_msg.get("tool_calls").is_none()
                    {
                        assistant_msg["content"] = serde_json::json!("");
                    }
                    api_messages.push(assistant_msg);
                } else {
                    api_messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": msg.get_all_text(),
                    }));
                }
            }
            Role::System => {
                api_messages.push(serde_json::json!({
                    "role": "system",
                    "content": msg.get_all_text(),
                }));
            }
        }
    }

    api_messages
}

#[async_trait::async_trait]
impl Provider for OpenAi {
    fn name(&self) -> &str {
        "openai"
    }

    fn context_window(&self, model: &str) -> u64 {
        match model {
            m if m.contains("gpt-4o") => 128_000,
            m if m.contains("gpt-4-turbo") => 128_000,
            m if m.contains("gpt-4") => 8_192,
            m if m.contains("gpt-3.5") => 16_385,
            _ => 128_000,
        }
    }

    fn capabilities(&self, _model: &str) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            thinking: false,
            system_prompt: true,
            caching: false,
        }
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionStream> {
        let model = if request.model.is_empty() {
            self.default_model.clone()
        } else {
            request.model.clone()
        };

        // Build OpenAI-format messages with proper tool use round-trips
        let mut api_messages: Vec<serde_json::Value> = Vec::new();

        if let Some(system) = &request.system {
            api_messages.push(serde_json::json!({
                "role": "system",
                "content": system,
            }));
        }

        api_messages.extend(convert_messages(&request.messages));

        let mut body = serde_json::json!({
            "model": model,
            "messages": api_messages,
            "max_tokens": request.max_tokens,
            "stream": true,
        });

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if !request.tools.is_empty() {
            let tools: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema,
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::Value::Array(tools);
        }

        let url = format!("{}/chat/completions", self.base_url);
        let auth_header = match &self.auth {
            Auth::ApiKey(key) | Auth::Bearer(key) => format!("Bearer {}", key),
            Auth::OAuth { token, .. } => format!("Bearer {}", token.access_token),
            Auth::Custom(_) => String::new(),
        };

        let (tx, rx) = mpsc::channel(256);

        let req = self
            .client
            .post(&url)
            .header("authorization", &auth_header)
            .header("content-type", "application/json")
            .json(&body)
            .build()
            .map_err(CerseiError::Http)?;

        let client = self.client.clone();

        tokio::spawn(async move {
            match client.execute(req).await {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status().as_u16();
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx
                            .send(StreamEvent::Error {
                                message: format!("HTTP {}: {}", status, body),
                            })
                            .await;
                        return;
                    }

                    let _ = tx
                        .send(StreamEvent::MessageStart {
                            id: String::new(),
                            model: String::new(),
                        })
                        .await;

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    // Content block tracking
                    let mut text_block_started = false;
                    let mut text_block_index: usize = 0;
                    let mut thinking_block_started = false;
                    let mut thinking_block_closed = false;
                    let mut thinking_block_index: usize = 0;
                    let mut next_block_index: usize = 0;
                    // Track streamed tool calls
                    let mut tool_acc = ToolCallAccumulator::new();
                    let mut tc_index_map: HashMap<u32, usize> = HashMap::new();
                    let mut final_stop_reason: Option<StopReason> = None;
                    let mut stream_usage: Option<Usage> = None;

                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));
                                while let Some(pos) = buffer.find('\n') {
                                    let line = buffer[..pos].to_string();
                                    buffer = buffer[pos + 1..].to_string();

                                    let data = match line.strip_prefix("data: ") {
                                        Some(d) => d.trim(),
                                        None => continue,
                                    };

                                    if data == "[DONE]" {
                                        // Close any open thinking block
                                        if thinking_block_started && !thinking_block_closed {
                                            let _ = tx
                                                .send(StreamEvent::ContentBlockStop {
                                                    index: thinking_block_index,
                                                })
                                                .await;
                                        }
                                        // Close any open text block
                                        if text_block_started {
                                            let _ = tx
                                                .send(StreamEvent::ContentBlockStop {
                                                    index: text_block_index,
                                                })
                                                .await;
                                        }
                                        // Close any open tool call blocks
                                        for (&tc_idx, &block_idx) in &tc_index_map {
                                            if let Some((_, _, ref args)) =
                                                tool_acc.calls.get(&tc_idx)
                                            {
                                                // Flush any remaining arguments
                                                if !args.is_empty() {
                                                    // Already flushed incrementally
                                                }
                                            }
                                            let _ = tx
                                                .send(StreamEvent::ContentBlockStop {
                                                    index: block_idx,
                                                })
                                                .await;
                                        }

                                        let stop_reason = final_stop_reason
                                            .unwrap_or(StopReason::EndTurn);
                                        let _ = tx
                                            .send(StreamEvent::MessageDelta {
                                                stop_reason: Some(stop_reason),
                                                usage: stream_usage.take(),
                                            })
                                            .await;
                                        let _ =
                                            tx.send(StreamEvent::MessageStop).await;
                                        return;
                                    }

                                    let json = match serde_json::from_str::<
                                        serde_json::Value,
                                    >(
                                        data
                                    ) {
                                        Ok(v) => v,
                                        Err(_) => continue,
                                    };

                                    let choice = &json["choices"][0];

                                    // Track finish_reason
                                    if let Some(fr) = choice["finish_reason"].as_str()
                                    {
                                        final_stop_reason = Some(match fr {
                                            "stop" => StopReason::EndTurn,
                                            "length" => StopReason::MaxTokens,
                                            "tool_calls" => StopReason::ToolUse,
                                            "content_filter" => {
                                                StopReason::ContentFilter
                                            }
                                            _ => StopReason::EndTurn,
                                        });
                                    }

                                    // Track usage if present (some providers include
                                    // it in the stream with stream_options)
                                    if let Some(usage_obj) = json.get("usage") {
                                        if usage_obj.is_object() {
                                            stream_usage = Some(Usage {
                                                input_tokens: usage_obj
                                                    ["prompt_tokens"]
                                                    .as_u64()
                                                    .unwrap_or(0),
                                                output_tokens: usage_obj
                                                    ["completion_tokens"]
                                                    .as_u64()
                                                    .unwrap_or(0),
                                                total_tokens: usage_obj
                                                    ["total_tokens"]
                                                    .as_u64()
                                                    .unwrap_or(0),
                                                cost_usd: None,
                                                provider_usage:
                                                    serde_json::Value::Null,
                                            });
                                        }
                                    }

                                    let delta = &choice["delta"];

                                    // Handle reasoning_content (Kimi/DeepSeek thinking)
                                    if let Some(reasoning) = delta["reasoning_content"].as_str() {
                                        if !reasoning.is_empty() {
                                            if !thinking_block_started {
                                                let _ = tx
                                                    .send(
                                                        StreamEvent::ContentBlockStart {
                                                            index: next_block_index,
                                                            block_type: "thinking".into(),
                                                            id: None,
                                                            name: None,
                                                        },
                                                    )
                                                    .await;
                                                thinking_block_index = next_block_index;
                                                next_block_index += 1;
                                                thinking_block_started = true;
                                            }
                                            let _ = tx
                                                .send(StreamEvent::ThinkingDelta {
                                                    index: thinking_block_index,
                                                    thinking: reasoning.to_string(),
                                                })
                                                .await;
                                        }
                                    }

                                    // Handle text content
                                    if let Some(content) = delta["content"].as_str() {
                                        if !content.is_empty() {
                                            // Close thinking block when text starts
                                            if thinking_block_started && !thinking_block_closed {
                                                let _ = tx
                                                    .send(StreamEvent::ContentBlockStop {
                                                        index: thinking_block_index,
                                                    })
                                                    .await;
                                                thinking_block_closed = true;
                                            }
                                            if !text_block_started {
                                                let _ = tx
                                                    .send(
                                                        StreamEvent::ContentBlockStart {
                                                            index: next_block_index,
                                                            block_type: "text".into(),
                                                            id: None,
                                                            name: None,
                                                        },
                                                    )
                                                    .await;
                                                text_block_index = next_block_index;
                                                next_block_index += 1;
                                                text_block_started = true;
                                            }
                                            let _ = tx
                                                .send(StreamEvent::TextDelta {
                                                    index: text_block_index,
                                                    text: content.to_string(),
                                                })
                                                .await;
                                        }
                                    }

                                    // Handle tool calls
                                    if let Some(tool_calls) =
                                        delta["tool_calls"].as_array()
                                    {
                                        // Close thinking/text blocks before first tool call
                                        if tool_acc.is_empty() {
                                            if thinking_block_started && !thinking_block_closed {
                                                let _ = tx
                                                    .send(StreamEvent::ContentBlockStop {
                                                        index: thinking_block_index,
                                                    })
                                                    .await;
                                                thinking_block_closed = true;
                                            }
                                            if text_block_started {
                                                let _ = tx
                                                    .send(StreamEvent::ContentBlockStop {
                                                        index: text_block_index,
                                                    })
                                                    .await;
                                                text_block_started = false;
                                            }
                                        }

                                        for tc in tool_calls {
                                            let tc_idx =
                                                tc["index"].as_u64().unwrap_or(0)
                                                    as u32;

                                            // New tool call? Emit ContentBlockStart
                                            if !tool_acc.started.contains(&tc_idx) {
                                                let id = tc["id"]
                                                    .as_str()
                                                    .unwrap_or("")
                                                    .to_string();
                                                let name = tc["function"]["name"]
                                                    .as_str()
                                                    .unwrap_or("")
                                                    .to_string();

                                                // If no text block was started, tool
                                                // calls start at index 0
                                                if next_block_index == 0 {
                                                    next_block_index = 0;
                                                }
                                                let block_idx = next_block_index;
                                                next_block_index += 1;

                                                tc_index_map
                                                    .insert(tc_idx, block_idx);
                                                tool_acc.calls.insert(
                                                    tc_idx,
                                                    (
                                                        id.clone(),
                                                        name.clone(),
                                                        String::new(),
                                                    ),
                                                );
                                                tool_acc.started.insert(tc_idx);

                                                let _ = tx
                                                    .send(
                                                        StreamEvent::ContentBlockStart {
                                                            index: block_idx,
                                                            block_type: "tool_use"
                                                                .into(),
                                                            id: Some(id),
                                                            name: Some(name),
                                                        },
                                                    )
                                                    .await;
                                            }

                                            // Accumulate arguments and emit deltas
                                            if let Some(args_chunk) =
                                                tc["function"]["arguments"].as_str()
                                            {
                                                if !args_chunk.is_empty() {
                                                    if let Some(entry) =
                                                        tool_acc.calls.get_mut(&tc_idx)
                                                    {
                                                        entry.2.push_str(args_chunk);
                                                    }
                                                    if let Some(&block_idx) =
                                                        tc_index_map.get(&tc_idx)
                                                    {
                                                        let _ = tx
                                                            .send(StreamEvent::InputJsonDelta {
                                                                index: block_idx,
                                                                partial_json:
                                                                    args_chunk
                                                                        .to_string(),
                                                            })
                                                            .await;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(StreamEvent::Error {
                                        message: e.to_string(),
                                    })
                                    .await;
                                return;
                            }
                        }
                    }

                    // Stream ended without [DONE] — close any open blocks
                    if thinking_block_started && !thinking_block_closed {
                        let _ = tx
                            .send(StreamEvent::ContentBlockStop { index: thinking_block_index })
                            .await;
                    }
                    if text_block_started {
                        let _ = tx
                            .send(StreamEvent::ContentBlockStop { index: text_block_index })
                            .await;
                    }
                    for (_, &block_idx) in &tc_index_map {
                        let _ = tx
                            .send(StreamEvent::ContentBlockStop { index: block_idx })
                            .await;
                    }
                    let stop_reason =
                        final_stop_reason.unwrap_or(StopReason::EndTurn);
                    let _ = tx
                        .send(StreamEvent::MessageDelta {
                            stop_reason: Some(stop_reason),
                            usage: stream_usage.take(),
                        })
                        .await;
                    let _ = tx.send(StreamEvent::MessageStop).await;
                }
                Err(e) => {
                    let _ = tx
                        .send(StreamEvent::Error {
                            message: e.to_string(),
                        })
                        .await;
                }
            }
        });

        Ok(CompletionStream::new(rx))
    }
}

// ─── Builder ─────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct OpenAiBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
}

impl OpenAiBuilder {
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn build(self) -> Result<OpenAi> {
        let auth = if let Some(key) = self.api_key {
            Auth::ApiKey(key)
        } else {
            return Err(CerseiError::Auth(
                "No API key provided. Set OPENAI_API_KEY or use .api_key()".into(),
            ));
        };

        Ok(OpenAi {
            auth,
            base_url: self.base_url.unwrap_or_else(|| OPENAI_API_BASE.to_string()),
            default_model: self.model.unwrap_or_else(|| "gpt-4o".to_string()),
            client: reqwest::Client::new(),
        })
    }
}
