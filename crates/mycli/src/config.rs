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
//! model = "kimi-k2.5"
//!
//! [cloud.kimi-think]
//! api_key = "sk-xxx"
//! base_url = "https://api.moonshot.ai/v1"
//! model = "kimi-k2.5"
//! max_tokens = 32768
//!
//! [cloud.deepseek]
//! api_key = "sk-xxx"
//! ```

use crate::Cli;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ─── Cloud profile ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CloudProfile {
    /// API key for this cloud provider
    pub api_key: String,
    /// Base URL override (otherwise uses built-in preset)
    pub base_url: String,
    /// Model name override (otherwise uses preset default)
    pub model: String,
    /// Max output tokens override
    pub max_tokens: Option<u32>,
    /// Max agent turns override
    pub max_turns: Option<u32>,
}

// ─── MCP server entry ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpEntry {
    /// Server name (for display and routing)
    pub name: String,
    /// Command to spawn (e.g. "npx", "python", "node")
    pub command: String,
    /// Arguments to the command
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables for the subprocess
    #[serde(default)]
    pub env: HashMap<String, String>,
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
    /// MCP servers
    #[serde(default)]
    pub mcp: Vec<McpEntry>,
    /// Named cloud provider profiles
    #[serde(default)]
    pub cloud: HashMap<String, CloudProfile>,
    /// Working directory (not serialized)
    #[serde(skip)]
    pub working_dir: PathBuf,
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
            mcp: Vec::new(),
            cloud: HashMap::new(),
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
            default_model: "kimi-k2.5",
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
            default_model: "gpt-4o",
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

        let max_tokens = profile
            .and_then(|p| p.max_tokens)
            .or(preset.as_ref().map(|p| p.max_tokens));
        let max_turns = profile.and_then(|p| p.max_turns);

        Some(ResolvedCloud {
            name: name.to_string(),
            base_url,
            model,
            api_key,
            max_tokens,
            max_turns,
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
    pub max_tokens: Option<u32>,
    pub max_turns: Option<u32>,
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
    if !overlay.mcp.is_empty() {
        base.mcp = overlay.mcp;
    }
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
            if config.model.is_empty() {
                config.model = resolved.model;
            }
            if let Some(mt) = resolved.max_tokens {
                config.max_tokens = mt;
            }
            if let Some(mt) = resolved.max_turns {
                config.max_turns = mt;
            }
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
