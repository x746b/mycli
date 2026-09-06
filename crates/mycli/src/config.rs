//! TOML configuration with layered loading.
//!
//! Priority (lowest -> highest):
//! 1. Hardcoded defaults (oMLX local)
//! 2. ~/.mycli/config.toml  (user global)
//! 3. .mycli/config.toml    (project local)
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
    /// Preserve unimplemented Codex options so they cannot be silently ignored.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl McpEntry {
    pub fn config_error(&self) -> Option<String> {
        if self.url.is_some() {
            return Some("HTTP MCP transport is not supported yet; use a command-based stdio server".into());
        }
        if !self.extra.is_empty() {
            return Some(format!("Unsupported MCP settings: {}", self.extra.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        if self.name.is_empty() || self.command.trim().is_empty() {
            return Some("MCP server needs a name and command".into());
        }
        None
    }

    pub fn server_config(&self) -> cersei_mcp::McpServerConfig {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        let mut config = cersei_mcp::McpServerConfig::stdio(&self.name, &self.command, &args);
        config.env = self.env.clone();
        config.cwd = self.cwd.clone();
        config
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
    /// Active persona: "code", "redteam", "blueteam", "data"
    #[serde(default = "default_persona")]
    pub persona: String,
    /// Stream the model's reasoning to the terminal. Toggle at runtime with
    /// Ctrl+O or `/thinking`.
    #[serde(default = "default_true")]
    pub show_thinking: bool,
    /// Reasoning effort for the active model, independent of display visibility.
    pub reasoning_effort: Option<String>,
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
            persona: "code".into(),
            show_thinking: true,
            reasoning_effort: None,
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
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
        assert_eq!(entries[0].server_config().cwd.as_deref(), Some("/tmp"));
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
        assert!(config.mcp_entries()[0].config_error().unwrap().contains("HTTP MCP"));
        assert!(config.mcp_entries()[1].config_error().unwrap().contains("disabled_tools"));
    }
}

// ─── Config directories ──────────────────────────────────────────────────

fn global_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mycli")
}

fn project_config_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".mycli")
}

pub fn history_path() -> PathBuf {
    global_config_dir().join("history")
}

// ─── Loading ──────────────────────────────────────────────────────────────

pub fn load() -> Config {
    let mut config = Config::default();

    // Layer 2: global
    if let Some(loaded) = load_toml(&global_config_dir().join("config.toml")) {
        merge(&mut config, loaded);
    }

    // Layer 3: project
    if let Some(loaded) = load_toml(&project_config_dir().join("config.toml")) {
        merge(&mut config, loaded);
    }

    // Layer 4: env vars
    apply_env(&mut config);

    config
}

fn load_toml(path: &Path) -> Option<Config> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
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
    if overlay.reasoning_effort.is_some() {
        base.reasoning_effort = overlay.reasoning_effort;
    }
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

pub fn apply_cli_overrides(cli: &Cli, config: &mut Config) {
    if let Some(m) = &cli.model {
        config.model = m.clone();
    }
    if let Some(cloud_name) = &cli.cloud {
        // Try config-defined cloud profile first, then built-in preset
        if let Some(resolved) = config.resolve_cloud(cloud_name) {
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
    }
    if let Some(effort) = &cli.reasoning {
        config.reasoning_effort = Some(effort.to_ascii_lowercase());
    }
}

/// Resolve tool tier. "auto" picks based on whether we're using a cloud provider.
pub fn resolve_tool_tier(config: &Config) -> &str {
    match config.tool_tier.as_str() {
        "simple" | "medium" | "full" => &config.tool_tier,
        _ => {
            // Auto: cloud = full, local = medium
            let is_local = config.provider == "omlx"
                || config.base_url.contains("127.0.0.1")
                || config.base_url.contains("localhost");
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
        apply_cli_overrides(&cli, &mut config);
        assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
        let cli = Cli::parse_from(["mycli", "--cloud", "openai", "--reasoning", "default"]);
        apply_cli_overrides(&cli, &mut config);
        assert_eq!(config.reasoning_effort.as_deref(), Some("default"));
        let cli = Cli::parse_from(["mycli", "--cloud", "gemini"]);
        apply_cli_overrides(&cli, &mut config);
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
        apply_cli_overrides(&Cli::parse_from(["mycli", "--cloud", "deepseek"]), &mut config);
        assert_eq!(config.model, "deepseek-v4-flash");
        assert_eq!(config.provider, "deepseek");
    }

    #[test]
    fn an_explicit_model_flag_still_wins() {
        let mut config = config_with_local_model();
        apply_cli_overrides(
            &Cli::parse_from(["mycli", "--cloud", "deepseek", "-m", "deepseek-v4-pro"]),
            &mut config,
        );
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
