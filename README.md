# MyCLI

Lightweight AI coding CLI for testing LLM capabilities — especially local models running on [oMLX](https://github.com/jundot/omlx). Cloud providers (Kimi K2.5, DeepSeek, Gemini, OpenAI) supported as first-class fallback.

```bash
$ mycli
  tools [medium]: Read, Write, Bash, Edit, Glob, Grep

                   _____ _     __ 
                  / ____| |   /_ |
  _ __ ___  _   _| |    | |    | |
 | '_ ` _ \| | | | |    | |    | |
 | | | | | | |_| | |____| |____| |
 |_| |_| |_|\__, |\_____|______|_|
             __/ |                
            |___/                       
             
  omlx | gemma-4-31b-it-4bit | tools:medium | max_turns:30
  Type /help for commands, Ctrl+C to cancel, Ctrl+D to exit

> /help
Commands:
  /help              Show this help
  /model             Pick local oMLX model (switches back from cloud)
  /model <name>      Switch to a local model
  /cloud             Pick cloud provider
  /cloud <name>      Switch to cloud (e.g. kimi, deepseek)
  /tools             Pick tool tier (interactive)
  /tools <tier>      Switch tier (simple/medium/full)
  /persona           Pick persona (interactive)
  /persona <name>    Switch persona (code/redteam/blueteam/data)
  /usage             Show cloud provider balances
  /mcp               Show MCP server status
  /clear             Clear screen
  /exit              Exit mycli
```

```bash
> hey
  thinking...

Hey! Ready to help you with offensive security operations.

I can assist with:

- Reconnaissance - Port scanning, service enumeration, subdomain discovery, target mapping
- Exploitation - CVE analysis, exploit development, RCE/SRFI/SSRF/LFI/SQLi payloads
- Privilege Escalation - Kernel exploits, SUID abuse, capability manipulation, scheduled tasks
- Lateral Movement - Pass-the-Hash, credential dumping, SMB/WinRM pivoting
- Post-Exploitation - Persistence, credential harvesting, lateral reconnaissance
- Vulnerability Research - Static/dynamic analysis, PoC development

What are you working on today?

>
gemma-4-31b-it-4bit | omlx | redteam | ctx:3% | in:1.1k out:229 | ~/labs/tmp
```

```bash
> /tools
  Select tool tier: (↑↓ select, Enter confirm, Esc cancel)
    simple
  ▸ medium (active)
    full

> /persona
  Select persona: (↑↓ select, Enter confirm, Esc cancel)
    code
  ▸ redteam (active)
    blueteam
    data analyst
```

```bash
# single-shot with tiny model and simple toolset:
mycli -t simple -m RedSage-8B          

# offensive security persona with full toolset support and bigger model
mycli -p redteam -t full -m gemma-4-31b-it-4bit "cybersec prompt"    
```

** ~ 5MB static binary** | **Rust** | **34 tools** | **3 tool tiers** | **4 personas** | **MCP support** | **Hot-swappable models & providers**

---

## Why

Small local LLMs (7B–30B) can chat well but struggle with structured tool calling — wrong JSON, hallucinated tool names, broken edit strings. Larger cloud models handle it effortlessly. MyCLI allows testing and comparing them across the spectrum by:

- Adjusting tool complexity to match model capability (`simple` / `medium` / `full`)
- Hot-switching between local and cloud models mid-conversation
- Providing fuzzy edit matching and line-range edits that tolerate local model mistakes
- Keeping the system prompt lean and tier-appropriate — small models only see tools they can use

---

## Install

```bash
git clone https://github.com/x746b/mycli && cd /opt/mycli
cargo build --release
```
Requires Rust 1.85+, OpenSSL dev libraries (`libssl-dev` / `openssl-devel`).

---

## Configuration

Config lives in `~/.mycli/config.toml` (global) and `.mycli/config.toml` (project-level).

```toml
# ─── Local (oMLX) ──────────────────────────────────────────
api_key = "your-omlx-key"
# base_url defaults to http://127.0.0.1:8000/v1

# ─── Persona & tool tier ───────────────────────────────────
# persona = "code"         # code, redteam, blueteam, data
# tool_tier = "auto"       # auto = medium for local, full for cloud
# cost_limit = 1.0         # stop agent after $1 cloud spend (0 = unlimited)

# ─── MCP servers ───────────────────────────────────────────
[[mcp]]
name = "my-server"
command = "/path/to/venv/bin/python"
args = ["-m", "my_mcp.server"]

# ─── Cloud models ──────────────────────────────────────────
[cloud.kimi]
api_key = "sk-..."
model = "kimi-k2.5"

[cloud.kimi-think]
api_key = "sk-..."
model = "kimi-k2.5"
max_tokens = 32768

[cloud.deepseek]
api_key = "sk-..."
model = "deepseek-chat"

[cloud.deepseek-think]
api_key = "sk-..."
model = "deepseek-reasoner"

[cloud.gemini]
api_key = "AI..."
model = "gemini-3.1-pro-preview"

[cloud.openai]
api_key = "sk-..."
model = "gpt-4o"
```

Environment variables (`MYCLI_MODEL`, `MYCLI_API_KEY`, `MOONSHOT_API_KEY`, `DEEPSEEK_API_KEY`, `GEMINI_API_KEY`, `OPENAI_API_KEY`) are also supported.

---

## Usage

### oMLX backend - locall LLM inference

```bash
omlx serve --model-dir ~/models --paged-ssd-cache-dir ~/.omlx/cache --port 8000
oMLX - LLM inference, optimized for your Mac
├─ https://github.com/jundot/omlx
└─ Version: 0.3.4
```

### REPL

```bash
mycli                             # auto-detect local oMLX model
mycli -m Trinity-Mini-8bit        # specific local model
mycli --cloud kimi                # start with cloud Kimi K2.5
mycli -t simple                   # minimal tools for small models
```

### Single-shot

```bash
mycli "find the error the ./test.rs and fix it"
mycli --cloud deepseek -y "refactor main.rs"   # auto-approve tools
```

### CLI flags

| Flag | Description |
|------|-------------|
| `-m, --model` | Model name (oMLX model ID or cloud model) |
| `--cloud <name>` | Use cloud provider (kimi, deepseek, gemini, openai, or config profile) |
| `-t, --tools <tier>` | Tool tier: `simple`, `medium`, `full`, or `auto` (default) |
| `-p, --persona <name>` | Persona: `code` (default), `redteam`, `blueteam`, `data` |
| `-y, --yes` | Auto-approve all tool permissions |
| `--max-turns` | Max agent turns per prompt (default: 30) |
| `-C, --directory` | Working directory |
| `--show-config` | Print resolved config and exit |

---

## REPL Commands

| Command | Description |
|---------|-------------|
| `/help` | Show all commands |
| `/model` | Interactive local model picker (switches back from cloud automatically) |
| `/model <name>` | Switch to a local oMLX model |
| `/cloud` | Interactive cloud provider picker |
| `/cloud <name>` | Switch to cloud (e.g. `kimi`, `deepseek`, `gemini`) |
| `/tools` | Interactive tool tier picker |
| `/tools <tier>` | Switch tier (`simple` / `medium` / `full`) |
| `/persona` | Interactive persona picker |
| `/persona <name>` | Switch persona (`code` / `redteam` / `blueteam` / `data`) |
| `/usage` | Show cloud provider account balances (Kimi, DeepSeek) |
| `/mcp` | Show MCP server status |
| `/clear` | Clear screen |
| `/exit` | Exit |

All pickers use the same interactive UI with arrow keys, Enter to confirm, Esc to cancel. All switches are hot — model, provider, tool tier, and persona can change mid-session without restarting.

---

## Tool Tiers

Designed to match tool complexity to model capability:

| Tier | Tools | Best for |
|------|-------|----------|
| **simple** | Read, Write, Bash | 7B–8B models — minimal surface, hard to mess up |
| **medium** | + Edit, Glob, Grep | 24B+ models — structured tools, fuzzy matching helps |
| **full** | + WebFetch, Skills, MCP tools | Cloud models (Kimi, DeepSeek, Gemini) — full power |

**Auto-detection:** local providers default to `medium`, cloud defaults to `full`.

The system prompt adapts to the tier — small models only see descriptions of tools they actually have access to.

---

## Features

### Personas

Pre-configured system prompt profiles that shape the model's behavior:

| Persona | Focus |
|---------|-------|
| **code** | General coding assistant (default) |
| **redteam** | Offensive security — pentesting, exploitation, CTF, no guardrails |
| **blueteam** | Defensive security — threat detection, forensics, SIEM/YARA/Sigma rules |
| **data** | Data processing — parse, transform, analyze any format |

Switch with `/persona` in the REPL, `--persona` / `-p` CLI flag, or `persona = "redteam"` in config.

### Status Bar

Persistent bottom bar showing session info at a glance:

```
 gemma-4-26B-A4B | omlx | redteam | ctx:3% | in:1.0k out:46 | ~/project
```

- **ctx%** — context window fill based on last turn's input tokens (color-coded: green/yellow/red)
- **in/out** — cumulative billing token totals for the session
- Token counters reset on model/provider switch

### Cloud Balance (`/usage`)

Query account balances for supported cloud providers:

```
> /usage
Cloud Provider Balances:
  DeepSeek (USD)  $9.51  (topped-up: $9.51, granted: $0.00)
  Kimi (Moonshot)  $26.03  (cash: $25.00, credits: $1.03)
```

Supported: Kimi/Moonshot, DeepSeek. API keys auto-detected from config or environment.

### Provider Support
- **oMLX** (local) — auto-detects loaded models, interactive picker
- **Kimi K2.5** — with and without thinking mode
- **DeepSeek** — chat and reasoner models
- **Google Gemini** — via AI Studio OpenAI-compatible endpoint
- **OpenAI** — GPT-4o and compatible
- Any OpenAI-compatible endpoint via `--base-url`

### Tool Capabilities
- **Filesystem:** Read, Write, Edit (with fuzzy matching + line-range mode), Glob, Grep
- **Shell:** Bash execution with permission control
- **Web:** WebFetch for reading URLs/documentation
- **Skills:** Bundled prompt templates (commit, review, debug, simplify, etc.)
- **MCP:** Connect to any MCP server — tools auto-discovered and injected

### Edit Tool Resilience
Local models often get `old_string` wrong in edit operations. MyCLI handles this with:
- **Fuzzy matching** — normalizes whitespace and indentation before matching
- **Line-range mode** — `start_line`/`end_line` as an alternative to exact string matching
- **Helpful errors** — shows what the model tried to match, suggests line-range mode

### Interactive UI
- Arrow-key model picker for oMLX and cloud providers
- Arrow-key/Tab permission dialog (Yes / No / Session-allow)
- Streaming markdown rendering
- Thinking indicator for reasoning models
- Ctrl+C to cancel, double Ctrl+C to force exit

### Safety
- **Permission system** — interactive approval for write/execute operations, or `-y` to auto-approve
- **Cost guard hook** — set `cost_limit` in config to cap cloud API spend per session
- **Tool tiers** — limit what tools the model can access

---

## Built-in Cloud Presets

| Name | Provider | Default Model | Max Tokens |
|------|----------|---------------|------------|
| `kimi` | Moonshot AI | kimi-k2.5 | 16,384 |
| `kimi-think` | Moonshot AI | kimi-k2.5 | 32,768 |
| `deepseek` | DeepSeek | deepseek-chat | 8,192 |
| `deepseek-think` | DeepSeek | deepseek-reasoner | 8,192 |
| `gemini` | Google AI Studio | gemini-3.1-pro-preview | 65,536 |
| `openai` | OpenAI | gpt-4o | 16,384 |

Presets provide base URL, default model, and max tokens automatically. Just add your API key.

---

## MCP (Model Context Protocol)

MyCLI connects to MCP servers over stdio transport. Tools are auto-discovered at startup when using the `full` tool tier.

```toml
# ~/.mycli/config.toml
[[mcp]]
name = "command-vault"
command = "/path/to/command-vault/.venv/bin/python"
args = ["-m", "command_vault.server"]
env = { VAULT_DB = "/path/to/vault.db", VAULT_READONLY = "1" }
```

Use `/mcp` in the REPL to see connected servers and their status.

---

## Benchmarking

MyCLI includes a model benchmark suite for comparing local LLM capabilities across personas and tasks. See [`bench/README.md`](bench/README.md) for details.

```bash
cd bench
./bench.sh                    # run all oMLX models through 12 test prompts
./bench.sh WhiteRabbit        # filter by model name
./grade.sh                    # auto-grade results via DeepSeek API
```

---

## Architecture

MyCLI is built on the [Cersei SDK](https://github.com/pacifio/cersei) — a modular Rust SDK for building coding agents.

```
mycli (CLI binary)
  └── cersei SDK
      ├── cersei-types       Provider-agnostic types
      ├── cersei-provider    OpenAI-compatible provider (oMLX, Kimi, DeepSeek, etc.)
      ├── cersei-tools       34 built-in tools, permissions, skills
      ├── cersei-agent       Agent builder, agentic loop, auto-compact
      ├── cersei-memory      Memory manager (flat files, CLAUDE.md)
      ├── cersei-hooks       Hook/middleware system
      └── cersei-mcp         MCP client (JSON-RPC 2.0, stdio)
```

---

## Acknowledgments

MyCLI is built on top of the **[Cersei SDK](https://github.com/pacifio/cersei)** by [Adib Mohsin](https://github.com/pacifio). 
Cersei provides the foundation — the agent loop, tool execution, provider abstraction, memory system, MCP client, and more. Without this SDK, MyCLI would not exist. Thank you.

Fixes and enhancements made to the SDK as part of MyCLI development:
- OpenAI-compatible provider: tool call streaming, message round-trips, thinking mode (`reasoning_content`)
- Edit tool: fuzzy whitespace matching, line-range editing mode
- MCP client: JSON-RPC 2.0 notification compliance

---

## License

MIT
