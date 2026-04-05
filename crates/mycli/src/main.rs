//! mycli — Lightweight AI coding CLI.
//! Local-first (oMLX), cloud-ready (Kimi, DeepSeek).

mod config;
mod render;
mod repl;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "mycli",
    about = "Lightweight AI coding CLI — local-first, cloud-ready",
    version,
    after_help = "Examples:\n  mycli                           Interactive REPL\n  mycli \"fix the tests\"           Single-shot mode\n  mycli -m RedSage-8B-8bit        Use specific model\n  mycli --cloud kimi              Use Kimi K2.5"
)]
pub struct Cli {
    /// Prompt to run in single-shot mode (omit for REPL)
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Model to use (default: from config or first available on oMLX)
    #[arg(short, long)]
    pub model: Option<String>,

    /// Use a cloud provider instead of local (kimi, deepseek, openai)
    #[arg(long)]
    pub cloud: Option<String>,

    /// Custom API base URL override
    #[arg(long)]
    pub base_url: Option<String>,

    /// API key override
    #[arg(long)]
    pub api_key: Option<String>,

    /// Max agent turns (default: 30)
    #[arg(long)]
    pub max_turns: Option<u32>,

    /// Tool tier: simple (Read/Write/Bash), medium (+ Edit/Glob/Grep),
    /// full (+ WebFetch/Skills). Auto-detected from provider if omitted.
    #[arg(long, short = 't', value_name = "TIER")]
    pub tools: Option<String>,

    /// Auto-approve all tool permissions (no prompts)
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Persona: code (default), redteam, blueteam, data
    #[arg(long, short = 'p', value_name = "PERSONA")]
    pub persona: Option<String>,

    /// Working directory override
    #[arg(short = 'C', long)]
    pub directory: Option<String>,

    /// Show config and exit
    #[arg(long)]
    pub show_config: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut cfg = config::load();
    config::apply_cli_overrides(&cli, &mut cfg);

    if cli.show_config {
        println!("{}", toml::to_string_pretty(&cfg)?);
        return Ok(());
    }

    repl::run(cli, cfg).await
}
