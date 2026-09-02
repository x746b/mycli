//! cersei-types: Provider-agnostic message types, errors, and content blocks
//! for the Cersei coding agent SDK.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

// ─── Roles ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

// ─── Content blocks ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: ToolResultContent,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
    Document {
        source: DocumentSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        citations: Option<CitationsConfig>,
    },
    /// Escape hatch for provider-specific block types not covered above.
    #[serde(other)]
    Opaque,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSource {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationsConfig {
    pub enabled: bool,
}

// ─── Messages ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// Provider-specific metadata (cache tokens, etc.)
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub provider_data: Value,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(content.into()),
            id: None,
            metadata: None,
        }
    }

    pub fn user_blocks(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Blocks(blocks),
            id: None,
            metadata: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(content.into()),
            id: None,
            metadata: None,
        }
    }

    pub fn assistant_blocks(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Blocks(blocks),
            id: None,
            metadata: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: MessageContent::Text(content.into()),
            id: None,
            metadata: None,
        }
    }

    /// Extract the first text content from this message.
    pub fn get_text(&self) -> Option<&str> {
        match &self.content {
            MessageContent::Text(t) => Some(t.as_str()),
            MessageContent::Blocks(blocks) => blocks.iter().find_map(|b| {
                if let ContentBlock::Text { text } = b {
                    Some(text.as_str())
                } else {
                    None
                }
            }),
        }
    }

    /// Collect all text content blocks into one concatenated string.
    pub fn get_all_text(&self) -> String {
        match &self.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
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
        }
    }

    pub fn get_tool_use_blocks(&self) -> Vec<&ContentBlock> {
        match &self.content {
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .collect(),
            _ => vec![],
        }
    }

    pub fn has_tool_use(&self) -> bool {
        !self.get_tool_use_blocks().is_empty()
    }

    pub fn content_blocks(&self) -> Vec<ContentBlock> {
        match &self.content {
            MessageContent::Text(t) => vec![ContentBlock::Text { text: t.clone() }],
            MessageContent::Blocks(b) => b.clone(),
        }
    }
}

// ─── Usage / Cost ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Provider-specific usage data (e.g. cache_creation_input_tokens for Anthropic)
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub provider_usage: Value,
}

impl Usage {
    pub fn total(&self) -> u64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens + self.output_tokens
        }
    }

    pub fn merge(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens = self.input_tokens + self.output_tokens;
        if let (Some(a), Some(b)) = (self.cost_usd, other.cost_usd) {
            self.cost_usd = Some(a + b);
        } else if other.cost_usd.is_some() {
            self.cost_usd = other.cost_usd;
        }
        // Provider fields do not add up — a duration or a cache count from two
        // responses cannot be summed into one meaningful number — so the most
        // recent report wins. A response carries exactly one usage object, so
        // for a single response this simply preserves it.
        if !other.provider_usage.is_null() {
            self.provider_usage = other.provider_usage.clone();
        }
    }

    /// Input tokens the provider served from a prompt cache, when it reports
    /// one. `input_tokens` minus this is the prompt it actually processed.
    ///
    /// OpenAI-compatible servers report `prompt_tokens_details.cached_tokens`;
    /// Anthropic reports `cache_read_input_tokens`.
    pub fn cached_input_tokens(&self) -> Option<u64> {
        self.provider_usage
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .or_else(|| self.provider_usage.get("cache_read_input_tokens"))
            .and_then(|v| v.as_u64())
    }

    /// Seconds the provider spent processing the prompt, when it reports it.
    pub fn prefill_seconds(&self) -> Option<f64> {
        self.positive_seconds("prompt_eval_duration")
    }

    /// Seconds the provider spent generating, when it reports it.
    pub fn decode_seconds(&self) -> Option<f64> {
        self.positive_seconds("generation_duration")
    }

    /// A reported duration, rejected when it is zero: servers round these to
    /// a couple of decimal places, so a short turn reports 0.0 and would give
    /// an infinite rate.
    fn positive_seconds(&self, key: &str) -> Option<f64> {
        self.provider_usage
            .get(key)
            .and_then(|v| v.as_f64())
            .filter(|secs| *secs > 0.0)
    }
}

#[cfg(test)]
mod usage_tests {
    use super::*;

    fn with(provider_usage: serde_json::Value) -> Usage {
        Usage {
            input_tokens: 613,
            output_tokens: 8,
            provider_usage,
            ..Default::default()
        }
    }

    #[test]
    fn reads_openai_and_anthropic_cache_counts() {
        let openai = with(serde_json::json!({"prompt_tokens_details": {"cached_tokens": 512}}));
        assert_eq!(openai.cached_input_tokens(), Some(512));

        let anthropic = with(serde_json::json!({"cache_read_input_tokens": 400}));
        assert_eq!(anthropic.cached_input_tokens(), Some(400));

        assert_eq!(with(serde_json::Value::Null).cached_input_tokens(), None);
    }

    /// A turn short enough to round to zero must not report an infinite rate.
    #[test]
    fn rejects_zero_durations() {
        let u = with(serde_json::json!({
            "prompt_eval_duration": 0.65,
            "generation_duration": 0.0,
        }));
        assert_eq!(u.prefill_seconds(), Some(0.65));
        assert_eq!(u.decode_seconds(), None);
    }

    /// Provider fields must survive the merge the stream accumulator does.
    #[test]
    fn merge_carries_provider_fields_forward() {
        let mut acc = Usage::default();
        acc.merge(&with(serde_json::json!({"generation_duration": 1.5})));
        assert_eq!(acc.decode_seconds(), Some(1.5));

        // A later report with nothing to say leaves the earlier one alone.
        acc.merge(&Usage::default());
        assert_eq!(acc.decode_seconds(), Some(1.5));
    }
}

// ─── Stop reasons ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    ContentFilter,
}

// ─── Tool definition (sent to providers) ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

// ─── Stream events ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum StreamEvent {
    MessageStart {
        id: String,
        model: String,
    },
    ContentBlockStart {
        index: usize,
        block_type: String,
        /// For tool_use blocks: the tool use ID. Default: None.
        #[allow(unused)]
        id: Option<String>,
        /// For tool_use blocks: the tool name. Default: None.
        #[allow(unused)]
        name: Option<String>,
    },
    TextDelta {
        index: usize,
        text: String,
    },
    InputJsonDelta {
        index: usize,
        partial_json: String,
    },
    ThinkingDelta {
        index: usize,
        thinking: String,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        stop_reason: Option<StopReason>,
        usage: Option<Usage>,
    },
    MessageStop,
    Error {
        message: String,
    },
    Ping,
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(thiserror::Error, Debug)]
pub enum CerseiError {
    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Provider error {status}: {message}")]
    ProviderStatus { status: u16, message: String },

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Permission denied: {0}")]
    Permission(String),

    #[error("Rate limit exceeded")]
    RateLimit {
        retry_after: Option<Duration>,
    },

    #[error("Context overflow: {used}/{limit} tokens")]
    ContextOverflow { used: u64, limit: u64 },

    #[error("Cancelled")]
    Cancelled,

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl CerseiError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            CerseiError::RateLimit { .. }
                | CerseiError::ProviderStatus { status: 429, .. }
                | CerseiError::ProviderStatus { status: 529, .. }
        )
    }

    pub fn is_context_limit(&self) -> bool {
        matches!(self, CerseiError::ContextOverflow { .. })
    }
}

pub type Result<T> = std::result::Result<T, CerseiError>;

// ─── Session info ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub content: String,
    pub relevance: f32,
    pub source: String,
}
