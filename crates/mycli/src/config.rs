//! TOML configuration with layered loading.
//!
//! Priority (lowest -> highest):
//! 1. Hardcoded defaults (oMLX local)
//! 2. ~/.config/mycli/config.toml  (user global)
//! 3. .config/mycli/config.toml    (project local)
//! 4. Environment variables  (MYCLI_MODEL, etc.)
//! 5. CLI flags
//!
//! Config example:
//! ```toml
//! api_key = "omlx-xxx"
//!
//! [cloud.kimi]
//! api_key = "sk-xxx"
//! model = "kimi-k3"
//!
//! [cloud.kimi-think]
//! api_key = "sk-xxx"
//! base_url = "https://api.moonshot.ai/v1"
//! model = "kimi-k3"
//! max_tokens = 32768
//!
//! [cloud.deepseek]
//! api_key = "sk-xxx"
//! ```

use crate::Cli;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

// ─── Cloud profile ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CloudProfile {
    /// API key for this cloud provider
    pub api_key: String,
    /// Admin key, used only for billing/usage queries. OpenAI's Costs API needs
    /// an `sk-admin-…` key with the `api.usage.read` scope; ordinary `sk-` keys
    /// get 403. Falls back to OPENAI_ADMIN_KEY when unset.
    pub admin_key: String,
    /// Credit top-up amount, in the provider's billing currency. OpenAI exposes
    /// no balance endpoint at all, so remaining credit can only be derived:
    /// this figure minus spend since `credits_since`. Purely informational.
    pub credits: Option<f64>,
    /// Date the `credits` top-up landed, `YYYY-MM-DD`. Spend is summed from here.
    pub credits_since: String,
    /// Base URL override (otherwise uses built-in preset)
    pub base_url: String,
    /// Model name override (otherwise uses preset default)
    pub model: String,
    /// Max output tokens override
    pub max_tokens: Option<u32>,
    /// Max agent turns override
    pub max_turns: Option<u32>,
    /// Context window in tokens. Set this for a cloud model whose name the
    /// built-in table does not recognise — without it the window is guessed
    /// from the model id, and an unrecognised id falls back to 32,768.
    ///
    /// Distinct from `max_tokens`, which caps a single response.
    pub context_window: Option<u64>,
    /// Model reasoning effort; omitted or "default" uses the provider default.
    pub reasoning_effort: Option<String>,
}

/// Named settings for an OpenAI-compatible local inference server.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LocalProfile {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub max_turns: Option<u32>,
    pub context_window: Option<u64>,
    pub reasoning_effort: Option<String>,
    /// Supported effort identifiers; omitted uses the built-in model catalog, [] disables levels.
    pub reasoning_levels: Option<Vec<String>>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub min_p: Option<f32>,
    /// Explicit model-level thinking control, separate from display visibility.
    pub thinking: Option<bool>,
    pub tool_tier: Option<String>,
    pub persona: Option<String>,
    pub show_thinking: Option<bool>,
    /// Enable the oMLX-specific search endpoint only on compatible servers.
    pub web_search: bool,
}

// ─── MCP server entry ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpEntry {
    /// Server name (for display and routing)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Command to spawn (e.g. "npx", "python", "node")
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// Arguments to the command
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables for the subprocess
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub http_headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env_http_headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token_env_var: Option<String>,
    /// Preserve unimplemented Codex options so they cannot be silently ignored.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl McpEntry {
    pub fn config_error(&self) -> Option<String> {
        if let Some(url) = &self.url {
            if !self.command.trim().is_empty() || !self.args.is_empty() || !self.env.is_empty() || self.cwd.is_some() {
                return Some("HTTP MCP cannot also specify command, args, env, or cwd".into());
            }
            if let Err(e) = self.resolved_headers().and_then(|h| cersei_mcp::http::HttpTransport::new(&cersei_mcp::expand_env_vars(url), &h).map(|_| ()).map_err(|e| e.to_string())) {
                return Some(e);
            }
        } else if !self.http_headers.is_empty() || !self.env_http_headers.is_empty() || self.bearer_token_env_var.is_some() {
            return Some("MCP HTTP headers require a url".into());
        }
        if !self.extra.is_empty() {
            return Some(format!("Unsupported MCP settings: {}", self.extra.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        if self.name.is_empty() || (self.url.is_none() && self.command.trim().is_empty()) {
            return Some("MCP server needs a name and command".into());
        }
        None
    }

    fn resolved_headers(&self) -> Result<HashMap<String, String>, String> {
        let mut headers: HashMap<String, String> = self.http_headers.iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.clone())).collect();
        if headers.len() != self.http_headers.len() { return Err("Duplicate MCP HTTP header".into()); }
        for (header, variable) in &self.env_http_headers {
            let value = std::env::var(variable).map_err(|_| format!("Missing MCP header environment variable: {variable}"))?;
            if headers.insert(header.to_ascii_lowercase(), value).is_some() {
                return Err("Duplicate MCP HTTP header".into());
            }
        }
        if let Some(variable) = &self.bearer_token_env_var {
            let value = std::env::var(variable).map_err(|_| format!("Missing MCP bearer environment variable: {variable}"))?;
            if headers.insert("authorization".into(), format!("Bearer {value}")).is_some() {
                return Err("Duplicate MCP authorization header".into());
            }
        }
        Ok(headers)
    }

    pub fn server_config(&self) -> Result<cersei_mcp::McpServerConfig, String> {
        if let Some(url) = &self.url {
            let mut config = cersei_mcp::McpServerConfig::http(&self.name, url);
            config.headers = self.resolved_headers()?;
            return Ok(config);
        }
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        let mut config = cersei_mcp::McpServerConfig::stdio(&self.name, &self.command, &args);
        config.env = self.env.clone();
        config.cwd = self.cwd.clone();
        Ok(config)
    }
}

// ─── Main config ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Model name (oMLX model ID or cloud model name)
    pub model: String,
    /// Provider: "omlx", or a cloud profile name
    pub provider: String,
    /// API base URL
    pub base_url: String,
    /// API key (for oMLX by default)
    pub api_key: String,
    /// Maximum agent turns per prompt
    pub max_turns: u32,
    /// Max output tokens per turn
    pub max_tokens: u32,
    /// Auto-approve permissions
    pub auto_approve: bool,
    /// Tool tier: "simple", "medium", "full", or "auto" (default)
    pub tool_tier: String,
    /// Cost limit in USD per session (0 = unlimited)
    pub cost_limit: f64,
    /// Context window in tokens; 0 asks the provider and falls back to a
    /// guess from the model name. A cloud profile's own setting wins over it.
    #[serde(default)]
    pub context_window: u64,
    /// MCP servers
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<McpEntry>,
    /// Codex-compatible named MCP server tables.
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpEntry>,
    /// Named cloud provider profiles
    #[serde(default)]
    pub cloud: HashMap<String, CloudProfile>,
    /// Named local model profiles, selected with /local or --local.
    pub local: BTreeMap<String, LocalProfile>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub min_p: Option<f32>,
    /// Explicit model-level thinking control, separate from display visibility.
    pub thinking: Option<bool>,
    /// Whether this local server offers the oMLX web-search extension.
    #[serde(default = "default_true")]
    pub web_search: bool,
    /// Active persona name from system-prompts.toml.
    #[serde(default = "default_persona")]
    pub persona: String,
    /// Stream the model's reasoning to the terminal. Toggle at runtime with
    /// Ctrl+O or `/thinking`.
    #[serde(default = "default_true")]
    pub show_thinking: bool,
    /// Reasoning effort for the active model, independent of display visibility.
    pub reasoning_effort: Option<String>,
    /// Supported efforts for the default local server; named local profiles override independently.
    pub reasoning_levels: Option<Vec<String>>,
    /// Working directory (not serialized)
    #[serde(skip)]
    pub working_dir: PathBuf,
}

fn default_persona() -> String {
    "code".into()
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: String::new(),
            provider: "omlx".into(),
            base_url: "http://127.0.0.1:8000/v1".into(),
            api_key: String::new(),
            max_turns: 30,
            max_tokens: 16384,
            auto_approve: false,
            tool_tier: "auto".into(),
            cost_limit: 0.0,
            context_window: 0,
            mcp: Vec::new(),
            mcp_servers: BTreeMap::new(),
            cloud: HashMap::new(),
            local: BTreeMap::new(),
            temperature: None,
            top_p: None,
            min_p: None,
            thinking: None,
            web_search: true,
            persona: "code".into(),
            show_thinking: true,
            reasoning_effort: None,
            reasoning_levels: None,
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

fn sensitive_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "CREDENTIAL",
        "AUTH",
    ]
    .iter()
    .any(|marker| upper.contains(marker))
}

impl Config {
    /// Return a display-safe copy for diagnostics. The live configuration is
    /// left untouched so callers cannot accidentally replace usable secrets.
    pub fn redacted(&self) -> Self {
        let mut safe = self.clone();
        if !safe.api_key.is_empty() {
            safe.api_key = "<redacted>".into();
        }
        for profile in safe.cloud.values_mut() {
            if !profile.api_key.is_empty() {
                profile.api_key = "<redacted>".into();
            }
            if !profile.admin_key.is_empty() {
                profile.admin_key = "<redacted>".into();
            }
        }
        for profile in safe.local.values_mut() {
            if let Some(key) = &mut profile.api_key {
                if !key.is_empty() {
                    *key = "<redacted>".into();
                }
            }
        }
        let redact_env = |entry: &mut McpEntry| {
            for value in entry.http_headers.values_mut() { *value = "<redacted>".into(); }
            if entry.url.is_some() { entry.url = Some("<redacted>".into()); }
            for (name, value) in &mut entry.env {
                if !value.is_empty() && sensitive_env_name(name) {
                    *value = "<redacted>".into();
                }
            }
        };
        for entry in &mut safe.mcp {
            redact_env(entry);
        }
        for entry in safe.mcp_servers.values_mut() {
            redact_env(entry);
        }
        safe
    }
}

/// Built-in presets for well-known providers (used when cloud profile
/// doesn't specify base_url or model).
struct BuiltinPreset {
    base_url: &'static str,
    default_model: &'static str,
    env_key: &'static str,
    max_tokens: u32,
}

fn builtin_preset(name: &str) -> Option<BuiltinPreset> {
    match name {
        "kimi" | "kimi-think" | "moonshot" => Some(BuiltinPreset {
            base_url: "https://api.moonshot.ai/v1",
            default_model: "kimi-k3",
            env_key: "MOONSHOT_API_KEY",
            max_tokens: 16384,
        }),
        "deepseek" => Some(BuiltinPreset {
            base_url: "https://api.deepseek.com/v1",
            default_model: "deepseek-chat",
            env_key: "DEEPSEEK_API_KEY",
            max_tokens: 8192,
        }),
        "deepseek-think" => Some(BuiltinPreset {
            base_url: "https://api.deepseek.com/v1",
            default_model: "deepseek-reasoner",
            env_key: "DEEPSEEK_API_KEY",
            max_tokens: 8192,
        }),
        "openai" => Some(BuiltinPreset {
            base_url: "https://api.openai.com/v1",
            default_model: "gpt-5.4",
            env_key: "OPENAI_API_KEY",
            max_tokens: 16384,
        }),
        "gemini" | "google" | "aistudio" => Some(BuiltinPreset {
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
            default_model: "gemini-3.1-pro-preview",
            env_key: "GEMINI_API_KEY",
            max_tokens: 65536,
        }),
        _ => None,
    }
}

impl Config {
    pub fn is_local(&self) -> bool {
        self.provider == "omlx"
            || self.provider.starts_with("local:")
            || self.base_url.contains("127.0.0.1")
            || self.base_url.contains("localhost")
    }

    pub fn reasoning_levels_override(&self) -> Option<&[String]> {
        if self.is_local() { self.reasoning_levels.as_deref() } else { None }
    }

    pub fn reasoning_choices<'a>(&'a self, model: &str) -> Vec<&'a str> {
        self.reasoning_levels_override().map(|items| items.iter().map(String::as_str).collect())
            .unwrap_or_else(|| cersei_provider::reasoning::levels(model).to_vec())
    }

    pub fn validate_reasoning(&self, model: &str, effort: &str) -> cersei_types::Result<()> {
        cersei_provider::reasoning::validate_with_levels(model, effort, self.reasoning_levels_override())
    }

    /// Reset settings that belong to a model before switching providers.
    pub fn restore_model_defaults(&mut self, defaults: &Config) {
        self.max_tokens = defaults.max_tokens;
        self.max_turns = defaults.max_turns;
        self.context_window = defaults.context_window;
        self.reasoning_effort = defaults.reasoning_effort.clone();
        self.reasoning_levels = defaults.reasoning_levels.clone();
        self.temperature = defaults.temperature;
        self.top_p = defaults.top_p;
        self.min_p = defaults.min_p;
        self.thinking = defaults.thinking;
        self.tool_tier = defaults.tool_tier.clone();
        self.persona = defaults.persona.clone();
        self.show_thinking = defaults.show_thinking;
        self.web_search = defaults.web_search;
    }

    pub fn with_local_profile(&self, name: &str, defaults: &Config) -> anyhow::Result<Self> {
        let profile = self.local.get(name).ok_or_else(|| anyhow::anyhow!(
            "Unknown local profile '{name}'. Add [local.{name}] to ~/.config/mycli/config.toml"
        ))?;
        if let Some(t) = profile.temperature {
            anyhow::ensure!(t.is_finite() && (0.0..=2.0).contains(&t),
                "Local profile '{name}': temperature must be between 0 and 2");
        }
        for (field, value) in [("top_p", profile.top_p.or(defaults.top_p)),
                               ("min_p", profile.min_p.or(defaults.min_p))] {
            if let Some(value) = value {
                anyhow::ensure!(value.is_finite() && (0.0..=1.0).contains(&value),
                    "Local profile '{name}': {field} must be between 0 and 1");
            }
        }
        anyhow::ensure!(profile.max_tokens != Some(0) && profile.max_turns != Some(0),
            "Local profile '{name}': max_tokens and max_turns must be positive");
        if let Some(tier) = &profile.tool_tier {
            anyhow::ensure!(["auto", "simple", "medium", "full"].contains(&tier.as_str()),
                "Local profile '{name}': tool_tier must be auto, simple, medium, or full");
        }
        let mut next = self.clone();
        next.restore_model_defaults(defaults);
        next.provider = format!("local:{name}");
        next.base_url = profile.base_url.as_deref().unwrap_or(&defaults.base_url)
            .trim_end_matches('/').to_string();
        anyhow::ensure!(next.base_url.starts_with("http://") || next.base_url.starts_with("https://"),
            "Local profile '{name}': base_url must start with http:// or https://");
        // Do not send the default server's credential to a different endpoint.
        next.api_key = profile.api_key.clone().unwrap_or_else(|| {
            if next.base_url == defaults.base_url.trim_end_matches('/') {
                defaults.api_key.clone()
            } else {
                String::new()
            }
        });
        next.model = profile.model.clone();
        next.max_tokens = profile.max_tokens.unwrap_or(defaults.max_tokens);
        next.max_turns = profile.max_turns.unwrap_or(defaults.max_turns);
        next.context_window = profile.context_window.unwrap_or(0);
        next.reasoning_effort = profile.reasoning_effort.clone();
        next.reasoning_levels = profile.reasoning_levels.clone();
        if let Some(levels) = &next.reasoning_levels {
            cersei_provider::reasoning::validate_level_names(levels)?;
        }
        next.temperature = profile.temperature.or(defaults.temperature);
        next.top_p = profile.top_p.or(defaults.top_p);
        next.min_p = profile.min_p.or(defaults.min_p);
        next.thinking = profile.thinking.or(defaults.thinking);
        if let Some(tier) = &profile.tool_tier { next.tool_tier = tier.clone(); }
        if let Some(persona) = &profile.persona { next.persona = persona.clone(); }
        next.show_thinking = profile.show_thinking.unwrap_or(defaults.show_thinking);
        next.web_search = profile.web_search;
        Ok(next)
    }

    /// Accept both formats. Named tables win for duplicates in the same file.
    pub fn mcp_entries(&self) -> Vec<McpEntry> {
        let mut servers: BTreeMap<String, McpEntry> = self.mcp.iter()
            .map(|entry| (entry.name.clone(), entry.clone())).collect();
        servers.extend(self.mcp_servers.clone());
        servers.into_iter().map(|(name, mut entry)| {
            entry.name = name;
            entry
        }).collect()
    }

    /// Resolve a cloud profile by name. Merges the profile's settings with
    /// built-in presets and environment variables.
    pub fn resolve_cloud(&self, name: &str) -> Option<ResolvedCloud> {
        let profile = self.cloud.get(name);
        let preset = builtin_preset(name);

        // Must have at least a profile or a preset
        if profile.is_none() && preset.is_none() {
            return None;
        }

        let base_url = match (&profile, &preset) {
            (Some(p), _) if !p.base_url.is_empty() => p.base_url.clone(),
            (_, Some(pre)) => pre.base_url.to_string(),
            _ => return None,
        };

        let model = match (&profile, &preset) {
            (Some(p), _) if !p.model.is_empty() => p.model.clone(),
            (_, Some(pre)) => pre.default_model.to_string(),
            _ => String::new(),
        };

        // API key: profile > env var > empty
        let api_key = match (&profile, &preset) {
            (Some(p), _) if !p.api_key.is_empty() => p.api_key.clone(),
            (_, Some(pre)) => std::env::var(pre.env_key).unwrap_or_default(),
            (Some(p), None) => p.api_key.clone(),
            _ => String::new(),
        };

        // Admin key: profile > OPENAI_ADMIN_KEY > empty. Billing queries only.
        let admin_key = match &profile {
            Some(p) if !p.admin_key.is_empty() => p.admin_key.clone(),
            _ => std::env::var("OPENAI_ADMIN_KEY").unwrap_or_default(),
        };

        let credits = profile.and_then(|p| p.credits);
        let credits_since = profile
            .map(|p| p.credits_since.clone())
            .unwrap_or_default();

        let max_tokens = profile
            .and_then(|p| p.max_tokens)
            .or(preset.as_ref().map(|p| p.max_tokens));
        let max_turns = profile.and_then(|p| p.max_turns);
        let context_window = profile.and_then(|p| p.context_window).filter(|w| *w > 0);

        Some(ResolvedCloud {
            name: name.to_string(),
            base_url,
            model,
            api_key,
            admin_key,
            credits,
            credits_since,
            max_tokens,
            max_turns,
            context_window,
            reasoning_effort: profile.and_then(|p| p.reasoning_effort.clone()),
        })
    }

    /// List all available cloud profiles (configured + built-in presets).
    pub fn available_clouds(&self) -> Vec<String> {
        let mut names: Vec<String> = self.cloud.keys().cloned().collect();
        // Add built-in presets that aren't already configured
        for builtin in &["kimi", "deepseek", "openai", "gemini"] {
            if !names.contains(&builtin.to_string()) {
                names.push(builtin.to_string());
            }
        }
        names.sort();
        names
    }
}

pub struct ResolvedCloud {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub admin_key: String,
    pub credits: Option<f64>,
    pub credits_since: String,
    pub max_tokens: Option<u32>,
    pub max_turns: Option<u32>,
    pub context_window: Option<u64>,
    pub reasoning_effort: Option<String>,
}

#[cfg(test)]
mod mcp_config_tests {
    use super::*;

    #[test]
    fn codex_tables_support_inline_and_nested_env() {
        let config: Config = toml::from_str(r#"
            [mcp_servers.command-vault]
            command = "python3"
            args = ["-m", "command_vault.server"]
            env = { VAULT_READONLY = "1" }
            cwd = "/tmp"
            [mcp_servers.second]
            command = "node"
            [mcp_servers.second.env]
            EXAMPLE = "value"
        "#).unwrap();
        let entries = config.mcp_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "command-vault");
        assert_eq!(entries[0].env["VAULT_READONLY"], "1");
        assert_eq!(entries[0].server_config().unwrap().cwd.as_deref(), Some("/tmp"));
        assert_eq!(entries[1].env["EXAMPLE"], "value");
        assert!(entries.iter().all(|entry| entry.enabled && entry.config_error().is_none()));
    }

    #[test]
    fn named_tables_override_legacy_duplicates_once() {
        let config: Config = toml::from_str(r#"
            [[mcp]]
            name = "example"
            command = "old"
            [mcp_servers.example]
            command = "new"
        "#).unwrap();
        assert_eq!(config.mcp_entries().len(), 1);
        assert_eq!(config.mcp_entries()[0].command, "new");
    }

    #[test]
    fn project_overrides_by_name_across_formats_and_can_disable() {
        let mut base: Config = toml::from_str(r#"
            [mcp_servers.example]
            command = "global"
            [mcp_servers.keep]
            command = "keep"
        "#).unwrap();
        merge(&mut base, toml::from_str(r#"
            [[mcp]]
            name = "example"
            command = "project"
        "#).unwrap());
        assert_eq!(base.mcp_entries()[0].command, "project");
        merge(&mut base, toml::from_str(r#"
            [mcp_servers.example]
            enabled = false
        "#).unwrap());
        assert_eq!(base.mcp_entries().len(), 2);
        assert!(!base.mcp_entries()[0].enabled);
        assert_eq!(base.mcp_entries()[1].command, "keep");
        let serialized = toml::to_string(&base).unwrap();
        assert!(!serialized.contains("[[mcp]]"));
        assert!(serialized.contains("[mcp_servers.example]"));
        assert_eq!(toml::from_str::<Config>(&serialized).unwrap().mcp_entries().len(), 2);
    }

    #[test]
    fn unsupported_codex_settings_are_retained_and_reported() {
        let config: Config = toml::from_str(r#"
            model = "still-loaded"
            [mcp_servers.docs]
            url = "https://example.invalid/mcp"
            [mcp_servers.filtered]
            command = "python3"
            disabled_tools = ["example"]
        "#).unwrap();
        assert_eq!(config.model, "still-loaded");
        assert!(config.mcp_entries()[0].config_error().is_none());
        assert_eq!(config.mcp_entries()[0].server_config().unwrap().server_type, "http");
        assert!(config.mcp_entries()[1].config_error().unwrap().contains("disabled_tools"));
    }
}

#[cfg(test)]
mod redaction_tests {
    use super::*;

    #[test]
    fn diagnostic_copy_redacts_keys_and_sensitive_mcp_environment() {
        let mut config = Config {
            api_key: "local-secret".into(),
            ..Config::default()
        };
        config.cloud.insert(
            "openai".into(),
            CloudProfile {
                api_key: "cloud-secret".into(),
                admin_key: "admin-secret".into(),
                ..CloudProfile::default()
            },
        );
        config.mcp_servers.insert(
            "lookup".into(),
            McpEntry {
                name: String::new(),
                command: "lookup".into(),
                args: vec![],
                env: HashMap::from([
                    ("NVD_API_KEY".into(), "nvd-secret".into()),
                    ("CACHE_DIR".into(), "/tmp/cache".into()),
                ]),
                cwd: None,
                enabled: true,
                url: None,
                http_headers: HashMap::new(), env_http_headers: HashMap::new(), bearer_token_env_var: None,
                extra: BTreeMap::new(),
            },
        );

        let safe = config.redacted();
        assert_eq!(safe.api_key, "<redacted>");
        assert_eq!(safe.cloud["openai"].api_key, "<redacted>");
        assert_eq!(safe.cloud["openai"].admin_key, "<redacted>");
        assert_eq!(
            safe.mcp_servers["lookup"].env["NVD_API_KEY"],
            "<redacted>"
        );
        assert_eq!(safe.mcp_servers["lookup"].env["CACHE_DIR"], "/tmp/cache");
        assert_eq!(config.api_key, "local-secret");
    }
}

// ─── Config directories ──────────────────────────────────────────────────

/// Use XDG paths on every platform, including macOS (not Application Support).
pub fn global_config_dir() -> PathBuf {
    config_dir_from(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from), dirs::home_dir())
}

fn config_dir_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    xdg.filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.unwrap_or_else(|| PathBuf::from(".")).join(".config"))
        .join("mycli")
}

pub fn legacy_config_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".mycli")
}

/// An existing preferred file wins even if invalid; do not silently load stale settings.
fn preferred_or_legacy(preferred: PathBuf, legacy: PathBuf) -> PathBuf {
    if preferred.exists() { preferred } else { legacy }
}

pub fn global_config_path() -> PathBuf {
    preferred_or_legacy(global_config_dir().join("config.toml"), legacy_config_dir().join("config.toml"))
}

pub fn project_file(root: &Path, name: &str) -> PathBuf {
    preferred_or_legacy(root.join(".config/mycli").join(name), root.join(".mycli").join(name))
}

pub fn history_path() -> PathBuf {
    global_config_dir().join("history")
}

pub fn history_read_path() -> PathBuf {
    preferred_or_legacy(history_path(), legacy_config_dir().join("history"))
}

// ─── Loading ──────────────────────────────────────────────────────────────

pub fn load() -> Config {
    let mut config = Config::default();

    // Layer 2: global
    if let Some(loaded) = load_toml(&global_config_path()) {
        merge(&mut config, loaded);
    }

    // Layer 3: project
    if let Some(loaded) = load_toml(&project_file(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), "config.toml")) {
        merge(&mut config, loaded);
    }

    // Layer 4: env vars
    apply_env(&mut config);

    config
}

fn load_toml(path: &Path) -> Option<Config> {
    let content = std::fs::read_to_string(path).ok()?;
    match toml::from_str(&content) {
        Ok(config) => Some(config),
        Err(_) => {
            eprintln!("Warning: could not parse {}; check configuration fields and types", path.display());
            None
        }
    }
}

fn merge(base: &mut Config, overlay: Config) {
    let overlay_mcp = overlay.mcp_entries();
    let defaults = Config::default();
    if !overlay.model.is_empty() && overlay.model != defaults.model {
        base.model = overlay.model;
    }
    if overlay.provider != defaults.provider {
        base.provider = overlay.provider;
    }
    if overlay.base_url != defaults.base_url {
        base.base_url = overlay.base_url;
    }
    if !overlay.api_key.is_empty() {
        base.api_key = overlay.api_key;
    }
    if overlay.max_turns != defaults.max_turns {
        base.max_turns = overlay.max_turns;
    }
    if overlay.max_tokens != defaults.max_tokens {
        base.max_tokens = overlay.max_tokens;
    }
    if overlay.auto_approve {
        base.auto_approve = true;
    }
    if overlay.tool_tier != defaults.tool_tier {
        base.tool_tier = overlay.tool_tier;
    }
    if overlay.cost_limit != defaults.cost_limit {
        base.cost_limit = overlay.cost_limit;
    }
    if overlay.reasoning_levels.is_some() {
        base.reasoning_levels = overlay.reasoning_levels;
    }
    if overlay.reasoning_effort.is_some() {
        base.reasoning_effort = overlay.reasoning_effort;
    }
    if overlay.context_window != defaults.context_window {
        base.context_window = overlay.context_window;
    }
    if overlay.persona != defaults.persona { base.persona = overlay.persona; }
    if !overlay.show_thinking { base.show_thinking = false; }
    if !overlay.web_search { base.web_search = false; }
    if overlay.temperature.is_some() { base.temperature = overlay.temperature; }
    if overlay.top_p.is_some() { base.top_p = overlay.top_p; }
    if overlay.min_p.is_some() { base.min_p = overlay.min_p; }
    if overlay.thinking.is_some() { base.thinking = overlay.thinking; }
    base.local.extend(overlay.local);
    // Merge whole server definitions by name, independently of input syntax.
    // Project entries (including enabled=false) replace global definitions.
    let mut servers = BTreeMap::new();
    for mut entry in base.mcp_entries().into_iter().chain(overlay_mcp) {
        let name = std::mem::take(&mut entry.name);
        servers.insert(name, entry);
    }
    base.mcp.clear();
    base.mcp_servers = servers;
    // Merge cloud profiles (overlay wins per-profile)
    for (name, profile) in overlay.cloud {
        base.cloud.insert(name, profile);
    }
}

fn apply_env(config: &mut Config) {
    if let Ok(v) = std::env::var("MYCLI_MODEL") {
        config.model = v;
    }
    if let Ok(v) = std::env::var("MYCLI_PROVIDER") {
        config.provider = v;
    }
    if let Ok(v) = std::env::var("MYCLI_BASE_URL") {
        config.base_url = v;
    }
    if let Ok(v) = std::env::var("MYCLI_API_KEY") {
        config.api_key = v;
    }
    if let Ok(v) = std::env::var("MYCLI_MAX_TURNS") {
        if let Ok(n) = v.parse() {
            config.max_turns = n;
        }
    }
    if config.api_key.is_empty() {
        if let Ok(v) = std::env::var("OMLX_API_KEY") {
            config.api_key = v;
        }
    }
}

pub fn apply_cli_overrides(cli: &Cli, config: &mut Config) -> anyhow::Result<()> {
    if let Some(name) = &cli.local {
        *config = config.with_local_profile(name, config)?;
    }
    if let Some(m) = &cli.model {
        config.model = m.clone();
    }
    if let Some(cloud_name) = &cli.cloud {
        // Try config-defined cloud profile first, then built-in preset
        if let Some(resolved) = config.resolve_cloud(cloud_name) {
            config.temperature = None;
            config.top_p = None;
            config.min_p = None;
            config.thinking = None;
            config.provider = resolved.name;
            config.base_url = resolved.base_url;
            config.api_key = resolved.api_key;
            // Only an explicit -m outranks the profile's model. Testing the
            // field for emptiness instead of testing the flag also let the
            // config file's top-level `model` — which names the *local* model
            // — leak onto every cloud profile, so `--cloud deepseek` asked
            // DeepSeek for a model only the local server has.
            if cli.model.is_none() {
                config.model = resolved.model;
            }
            if let Some(mt) = resolved.max_tokens {
                config.max_tokens = mt;
            }
            if let Some(mt) = resolved.max_turns {
                config.max_turns = mt;
            }
            // The profile's window, or auto-detection. The top-level setting
            // describes the default provider, so it does not follow you onto a
            // cloud one — put a cloud model's window in its own profile.
            config.context_window = resolved.context_window.unwrap_or(0);
            config.reasoning_effort = resolved.reasoning_effort;
            config.reasoning_levels = None;
        } else {
            eprintln!(
                "Warning: unknown cloud profile '{}'. Available: {}",
                cloud_name,
                config.available_clouds().join(", ")
            );
            config.provider = cloud_name.clone();
        }
    }
    if let Some(u) = &cli.base_url {
        config.base_url = u.clone();
    }
    if let Some(k) = &cli.api_key {
        config.api_key = k.clone();
    }
    if let Some(n) = cli.max_turns {
        config.max_turns = n;
    }
    if cli.yes {
        config.auto_approve = true;
    }
    if let Some(dir) = &cli.directory {
        config.working_dir = PathBuf::from(dir);
    }
    if let Some(tier) = &cli.tools {
        config.tool_tier = tier.clone();
    }
    if let Some(p) = &cli.persona {
        config.persona = p.clone();
    }
    if cli.no_thinking {
        config.show_thinking = false;
        config.thinking = Some(false);
    }
    if let Some(effort) = &cli.reasoning {
        config.reasoning_effort = Some(effort.to_ascii_lowercase());
    }
    Ok(())
}

/// Resolve tool tier. "auto" picks based on whether we're using a cloud provider.
pub fn resolve_tool_tier(config: &Config) -> &str {
    match config.tool_tier.as_str() {
        "simple" | "medium" | "full" => &config.tool_tier,
        _ => {
            // Auto: cloud = full, local = medium
            let is_local = config.is_local();
            if is_local { "medium" } else { "full" }
        }
    }
}

#[cfg(test)]
mod cloud_override_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn reasoning_profile_default_and_cli_precedence() {
        let mut config: Config = toml::from_str(r#"
            reasoning_effort = "low"
            [cloud.openai]
            reasoning_effort = "high"
        "#).unwrap();
        let cli = Cli::parse_from(["mycli", "--cloud", "openai"]);
        apply_cli_overrides(&cli, &mut config).unwrap();
        assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
        let cli = Cli::parse_from(["mycli", "--cloud", "openai", "--reasoning", "default"]);
        apply_cli_overrides(&cli, &mut config).unwrap();
        assert_eq!(config.reasoning_effort.as_deref(), Some("default"));
        let cli = Cli::parse_from(["mycli", "--cloud", "gemini"]);
        apply_cli_overrides(&cli, &mut config).unwrap();
        assert_eq!(config.reasoning_effort, None);
    }

    #[test]
    fn omitted_effort_does_not_erase_a_configured_default() {
        let mut config = Config::default();
        merge(&mut config, toml::from_str("reasoning_effort = 'high'").unwrap());
        merge(&mut config, Config::default());
        assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
        merge(&mut config, toml::from_str("reasoning_effort = 'default'").unwrap());
        assert_eq!(config.reasoning_effort.as_deref(), Some("default"));
    }

    fn config_with_local_model() -> Config {
        let mut config = Config::default();
        config.model = "Qwen3.6-35B-A3B-8bit".into();
        config.cloud.insert(
            "deepseek".into(),
            CloudProfile {
                api_key: "k".into(),
                model: "deepseek-v4-flash".into(),
                ..Default::default()
            },
        );
        config
    }

    #[test]
    fn a_cloud_profile_replaces_the_local_model() {
        let mut config = config_with_local_model();
        apply_cli_overrides(&Cli::parse_from(["mycli", "--cloud", "deepseek"]), &mut config).unwrap();
        assert_eq!(config.model, "deepseek-v4-flash");
        assert_eq!(config.provider, "deepseek");
    }

    #[test]
    fn an_explicit_model_flag_still_wins() {
        let mut config = config_with_local_model();
        apply_cli_overrides(
            &Cli::parse_from(["mycli", "--cloud", "deepseek", "-m", "deepseek-v4-pro"]),
            &mut config,
        ).unwrap();
        assert_eq!(config.model, "deepseek-v4-pro");
    }
}

#[cfg(test)]
mod context_window_tests {
    use super::*;

    fn config_with(profile: CloudProfile) -> Config {
        let mut config = Config::default();
        config.cloud.insert("openai".into(), profile);
        config
    }

    #[test]
    fn a_profile_window_is_resolved() {
        let config = config_with(CloudProfile {
            api_key: "k".into(),
            model: "gpt-5.6-luna".into(),
            context_window: Some(400_000),
            ..Default::default()
        });
        let resolved = config.resolve_cloud("openai").unwrap();
        assert_eq!(resolved.context_window, Some(400_000));
    }

    /// Zero means "work it out", not "a window of nothing" — treating it as a
    /// real value would make every rate and warning divide by zero.
    #[test]
    fn zero_is_not_a_window() {
        let config = config_with(CloudProfile {
            api_key: "k".into(),
            context_window: Some(0),
            ..Default::default()
        });
        assert_eq!(config.resolve_cloud("openai").unwrap().context_window, None);

        let config = config_with(CloudProfile { api_key: "k".into(), ..Default::default() });
        assert_eq!(config.resolve_cloud("openai").unwrap().context_window, None);
    }
}

#[cfg(test)]
mod local_profile_tests {
    use super::*;
    use clap::Parser;

    fn configured() -> Config {
        toml::from_str(r#"
            api_key = "default-secret"
            base_url = "http://localhost:9000/v1"
            model = "default-model"
            max_tokens = 16000
            max_turns = 20
            context_window = 100000
            [local.ds4]
            base_url = "http://mac:8000/v1/"
            api_key = "ds4-secret"
            model = "deepseek-reasoner"
            context_window = 32768
            max_tokens = 8192
            max_turns = 10
            reasoning_effort = "max"
            temperature = 0.4
            top_p = 0.95
            min_p = 0.05
            thinking = false
            tool_tier = "full"
            persona = "data"
            show_thinking = false
            [local.glm]
            model = "glm-5.2"
            [local.remote]
            base_url = "http://another-server:8000/v1"
            [local.noauth]
            api_key = ""
            [cloud.ds4]
            base_url = "https://example.invalid/v1"
            model = "cloud-model"
        "#).unwrap()
    }

    #[test]
    fn local_profiles_apply_settings_and_remain_local_on_lan() {
        let defaults = configured();
        let config = defaults.with_local_profile("ds4", &defaults).unwrap();
        assert_eq!(config.provider, "local:ds4");
        assert!(config.is_local());
        assert_eq!(config.base_url, "http://mac:8000/v1");
        assert_eq!(config.api_key, "ds4-secret");
        assert_eq!(config.model, "deepseek-reasoner");
        assert_eq!((config.max_tokens, config.max_turns, config.context_window), (8192, 10, 32768));
        assert_eq!(config.reasoning_effort.as_deref(), Some("max"));
        assert_eq!(config.temperature, Some(0.4));
        assert_eq!(config.top_p, Some(0.95));
        assert_eq!(config.min_p, Some(0.05));
        assert_eq!(config.thinking, Some(false));
        assert_eq!(resolve_tool_tier(&config), "full");
        assert_eq!(config.persona, "data");
        assert!(!config.show_thinking);
        assert!(!config.web_search);
        assert_eq!(defaults.resolve_cloud("ds4").unwrap().model, "cloud-model");
    }

    #[test]
    fn switching_resets_model_settings_and_inherits_default_endpoint() {
        let defaults = configured();
        let first = defaults.with_local_profile("ds4", &defaults).unwrap();
        let second = first.with_local_profile("glm", &defaults).unwrap();
        assert_eq!(second.base_url, defaults.base_url);
        assert_eq!(second.api_key, "default-secret");
        assert_eq!((second.max_tokens, second.max_turns), (16000, 20));
        assert_eq!(second.context_window, 0);
        assert_eq!(second.reasoning_effort, None);
        assert_eq!(second.temperature, None);
        assert_eq!(second.top_p, None);
        assert_eq!(second.min_p, None);
        assert_eq!(second.thinking, None);
        assert_eq!(second.persona, "code");
        assert!(second.show_thinking);
        assert_eq!(resolve_tool_tier(&second), "medium");
    }

    #[test]
    fn credentials_are_redacted_and_not_inherited_across_endpoints() {
        let defaults = configured();
        for name in ["remote", "noauth"] {
            assert!(defaults.with_local_profile(name, &defaults).unwrap().api_key.is_empty());
        }
        let text = toml::to_string(&defaults.redacted()).unwrap();
        assert!(!text.contains("default-secret"));
        assert!(!text.contains("ds4-secret"));
        assert_eq!(defaults.local["ds4"].api_key.as_deref(), Some("ds4-secret"));
    }

    #[test]
    fn project_profiles_replace_global_definitions_by_name() {
        let mut config = configured();
        merge(&mut config, toml::from_str(r#"
            [local.ds4]
            model = "deepseek-chat"
            web_search = true
        "#).unwrap());
        assert_eq!(config.local.len(), 4);
        let selected = config.with_local_profile("ds4", &config).unwrap();
        assert_eq!(selected.model, "deepseek-chat");
        assert_eq!(selected.max_tokens, 16000);
        assert!(selected.web_search);
    }

    #[test]
    fn cli_flags_override_local_profile_and_reject_conflicting_providers() {
        let mut config = configured();
        apply_cli_overrides(&Cli::parse_from([
            "mycli", "--local", "ds4", "--model", "deepseek-chat", "--max-turns", "5",
            "--base-url", "http://localhost:1234/v1", "--api-key", "override",
            "--reasoning", "none", "--tools", "simple", "--persona", "math",
        ]), &mut config).unwrap();
        assert_eq!(config.model, "deepseek-chat");
        assert_eq!(config.base_url, "http://localhost:1234/v1");
        assert_eq!(config.api_key, "override");
        assert_eq!(config.max_turns, 5);
        assert_eq!(config.reasoning_effort.as_deref(), Some("none"));
        assert_eq!(config.tool_tier, "simple");
        assert_eq!(config.persona, "math");
        assert!(Cli::try_parse_from(["mycli", "--local", "ds4", "--cloud", "ds4"]).is_err());
        let before = config.model.clone();
        assert!(apply_cli_overrides(&Cli::parse_from(["mycli", "--local", "missing"]), &mut config).is_err());
        assert_eq!(config.model, before);
    }

    #[test]
    fn invalid_profile_settings_are_rejected() {
        for setting in ["temperature = -0.1", "temperature = 2.1", "temperature = nan",
                        "top_p = -0.1", "top_p = 1.1", "top_p = nan",
                        "min_p = -0.1", "min_p = 1.1", "min_p = inf",
                        "max_tokens = 0", "max_turns = 0", "tool_tier = 'invalid'",
                        "base_url = 'not-a-url'"] {
            let config: Config = toml::from_str(&format!("[local.bad]\n{setting}")).unwrap();
            assert!(config.with_local_profile("bad", &config).is_err(), "{setting}");
        }
    }
}

#[cfg(test)]
mod http_config_tests {
    use super::*;
    #[test]
    fn headers_are_redacted_and_bad_configs_fail_closed() {
        let config: Config = toml::from_str(r#"[mcp_servers.remote]
url = "http://localhost/mcp?key=secret"
http_headers = { Authorization = "Bearer secret" }
"#).unwrap();
        let text = toml::to_string(&config.redacted()).unwrap();
        assert!(!text.contains("secret"));
        assert!(config.mcp_entries()[0].config_error().is_none());
        let mut entry = config.mcp_entries()[0].clone();
        entry.command = "python".into();
        assert!(entry.config_error().is_some());
        entry.command.clear();
        entry.bearer_token_env_var = Some("MYCLI_MISSING_TEST_TOKEN_987".into());
        assert!(entry.config_error().is_some());
        assert!(entry.server_config().is_err());
    }
}

#[cfg(test)]
mod config_path_tests {
    use super::*;

    #[test]
    fn xdg_is_portable_and_requires_absolute_path() {
        let home = PathBuf::from("/home/test");
        assert_eq!(config_dir_from(None, Some(home.clone())), home.join(".config/mycli"));
        assert_eq!(config_dir_from(Some("/custom".into()), Some(home.clone())), PathBuf::from("/custom/mycli"));
        assert_eq!(config_dir_from(Some("relative".into()), Some(home.clone())), home.join(".config/mycli"));
        assert_eq!(config_dir_from(Some("".into()), Some(home.clone())), home.join(".config/mycli"));
    }

    #[test]
    fn preferred_project_files_win_without_merging_legacy_settings() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join(".mycli");
        let new = root.path().join(".config/mycli");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("config.toml"), "model='legacy'").unwrap();
        assert_eq!(project_file(root.path(), "config.toml"), old.join("config.toml"));
        std::fs::write(new.join("config.toml"), "model='new'").unwrap();
        assert_eq!(project_file(root.path(), "config.toml"), new.join("config.toml"));
        assert_eq!(load_toml(&project_file(root.path(), "config.toml")).unwrap().model, "new");
        std::fs::write(old.join("instructions.md"), "legacy instructions").unwrap();
        assert_eq!(project_file(root.path(), "instructions.md"), old.join("instructions.md"));
        std::fs::write(new.join("instructions.md"), "new instructions").unwrap();
        assert_eq!(project_file(root.path(), "instructions.md"), new.join("instructions.md"));
    }
}

#[cfg(test)]
mod local_reasoning_tests {
    use super::*;

    #[test]
    fn named_profiles_replace_default_server_levels_without_leaking() {
        let config: Config = toml::from_str(r#"
reasoning_levels = ["fast", "thorough"]
[local.custom]
model = "custom-model"
reasoning_levels = ["brief", "deep"]
reasoning_effort = "deep"
[local.builtin]
model = "Qwen-Cold-Fusion"
[local.disabled]
model = "Qwen-Cold-Fusion"
reasoning_levels = []
"#).unwrap();
        assert_eq!(config.reasoning_choices("unknown"), ["fast", "thorough"]);
        let custom = config.with_local_profile("custom", &config).unwrap();
        assert_eq!(custom.reasoning_choices("custom-model"), ["brief", "deep"]);
        assert!(custom.validate_reasoning("custom-model", "deep").is_ok());
        assert!(custom.validate_reasoning("custom-model", "high").is_err());
        let builtin = custom.with_local_profile("builtin", &config).unwrap();
        assert_eq!(builtin.reasoning_choices("Qwen-Cold-Fusion"), ["low", "medium", "xhigh", "einstein", "spoon"]);
        assert_eq!(builtin.reasoning_effort, None);
        let disabled = builtin.with_local_profile("disabled", &config).unwrap();
        assert!(disabled.reasoning_choices("Qwen-Cold-Fusion").is_empty());
        assert!(disabled.validate_reasoning("Qwen-Cold-Fusion", "spoon").is_err());
        let mut cloud = custom;
        cloud.provider = "deepseek".into();
        cloud.base_url = "https://api.deepseek.com/v1".into();
        assert_eq!(cloud.reasoning_choices("deepseek-flash"), ["none", "low", "high", "max"]);
    }

    #[test]
    fn config_merge_can_disable_levels_and_cloud_selection_clears_local_override() {
        let mut config = Config::default();
        merge(&mut config, toml::from_str("reasoning_levels=['fast']").unwrap());
        assert_eq!(config.reasoning_choices("unknown"), ["fast"]);
        merge(&mut config, toml::from_str("reasoning_levels=[]").unwrap());
        assert!(config.reasoning_choices("Qwen-Cold-Fusion").is_empty());
        use clap::Parser;
        let cli = crate::Cli::parse_from(["mycli", "--cloud", "deepseek"]);
        apply_cli_overrides(&cli, &mut config).unwrap();
        assert!(config.reasoning_levels.is_none());
    }
}
