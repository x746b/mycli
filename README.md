# MyCLI

Lightweight AI coding CLI for testing LLM capabilities — especially local models running on [oMLX](https://github.com/jundot/omlx). Cloud providers (Kimi, DeepSeek, Gemini, OpenAI) supported as first-class fallback.

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
             
  omlx | mlx-community_Qwen3.8-27B-mxfp8 | tools:medium | max_turns:30
  Type /help for commands, Ctrl+C to cancel, Ctrl+D to exit

> /persona
  Select persona: (↑↓ select, Enter confirm, Esc cancel)
    code
  ▸ redteam (active)
    blueteam
    data
    math
    agentic

> hey
  thinking...

Hey! Ready to help you with offensive security operations.
What are you working on today?

> mlx-community_Qwen3.8-27B-mxfp8 | omlx | redteam | ctx:3% | in:1.1k out:229 | ~/labs/tmp
```

```bash
# single-shot with tiny model and simple toolset:
mycli -t simple -m RedSage-8B          

# offensive security persona with full toolset support and bigger model
mycli -p redteam -t full -m mlx-community_Qwen3.8-27B-mxfp8 "cybersec prompt"    
```

** ~ 5MB static binary** | **Rust** | **34 tools** | **3 tool tiers** | **6 personas** | **MCP support** | **Hot-swappable models & providers**

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
git clone https://github.com/x746b/mycli && cd mycli
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
# model = "mlx-community_Qwen3.8-27B-mxfp8"   # empty/unset = auto-detect first loaded

# ─── Persona & tool tier ───────────────────────────────────
# persona = "code"         # code, redteam, blueteam, data, math, agentic
# tool_tier = "auto"       # auto = medium for local, full for cloud
# cost_limit = 1.0         # stop agent after $1 cloud spend (0 = unlimited)

# ─── MCP servers ───────────────────────────────────────────
[[mcp]]
name = "command-vault"
command = "/path/to/venv/bin/python"
args = ["-m", "command_vault.server"]
env = { VAULT_DB = "/path/to/vault.db", VAULT_READONLY = "1" }

# ─── Cloud models ──────────────────────────────────────────
[cloud.kimi]
api_key = "sk-..."
model = "kimi-k3"

[cloud.openai]
api_key = "sk-..."
model = "gpt-5.4"
# admin_key = "sk-admin-..."      # /usage spend; needs api.usage.read scope
# credits = 50.00                 # optional, credit balance
# credits_since = "2026-08-01"    # optional, date that balance was true
```

Same shape for `kimi-think`, `deepseek`, `deepseek-think` and `gemini`; add
`max_tokens` to any profile to override. `model` and `max_tokens` override the
built-in preset defaults — that is how you run a newer model than the preset ships with.

Environment variables (`MYCLI_MODEL`, `MYCLI_API_KEY`, `MOONSHOT_API_KEY`, `DEEPSEEK_API_KEY`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `OPENAI_ADMIN_KEY`) are also supported.

---

## Usage

### oMLX backend — local LLM inference

```bash
omlx serve --model-dir ~/AI/models --paged-ssd-cache-dir ~/.omlx/cache --port 8000
oMLX - LLM inference, optimized for your Mac
├─ https://github.com/jundot/omlx
└─ Version: 0.6.1
```

### REPL

```bash
mycli                             # auto-detect local oMLX model
mycli -m Trinity-Mini-8bit        # specific local model
mycli --cloud kimi                # start with cloud Kimi
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
| `-p, --persona <name>` | Persona: `code` (default), `redteam`, `blueteam`, `data`, `math`, `agentic` |
| `-y, --yes` | Auto-approve all tool permissions |
| `--no-thinking` | Start with the model's reasoning hidden (Ctrl+O toggles it) |
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
| `/persona <name>` | Switch persona (`code` / `redteam` / `blueteam` / `data` / `math` / `agentic`) |
| `/usage` | Show cloud balances / spend (Kimi, DeepSeek, OpenAI) |
| `/mcp` | Show MCP server status |
| `/thinking` | Toggle reasoning display (same as Ctrl+O) |
| `/thinking on\|off` | Force reasoning on or off |
| `/thinking last` | Reprint the last reasoning block |
| `/clear` | Clear screen |
| `/exit` | Exit |

### Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Esc` | Interrupt the running turn, at any point during generation |
| `Ctrl+O` | Toggle reasoning display (works at the prompt *and* mid-turn) |
| `Ctrl+C` | Interrupt the current turn (twice in quick succession to force exit) |
| `Ctrl+D` | Exit |
| `Tab` | Complete slash commands |
| `←` `→` / `Tab` | Move between options in an approval dialog |
| `Enter` / `Esc` | Confirm / deny an approval dialog |

All pickers use the same interactive UI with arrow keys, Enter to confirm, Esc to cancel. All switches are hot — model, provider, tool tier, and persona can change mid-session without restarting.

---

## Tool Tiers

Designed to match tool complexity to model capability:

| Tier | Tools | Best for |
|------|-------|----------|
| **simple** | Read, Write, Bash | 7B–8B models — minimal surface, hard to mess up |
| **medium** | + Edit, Glob, Grep | 24B+ models — structured tools, fuzzy matching helps |
| **full** | + WebFetch, Skills, MCP tools | bigger local and cloud models — full power |

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
| **math** | Mathematics and cryptography — number theory, modular arithmetic, RSA/ECC/AES, step-by-step proofs |
| **agentic** | Strict instruction following — tool use, structured output, constraint compliance, format adherence |

Switch with `/persona` in the REPL, `--persona` / `-p` CLI flag, or `persona = "redteam"` in config.

### Reasoning Display

Models that emit reasoning (Kimi, DeepSeek, and local models that return
`reasoning_content`) have it streamed inline, dimmed behind a gutter:

```
  ✻ Thinking
  │ Recursive-descent parser. Tokenizer first, then one function per
  │ precedence level, so `*` binds before `+`.
```

Press **Ctrl+O** to collapse it to a single live counter
(`✻ Thinking… 412 chars · ctrl+o to show`) and again to bring it back — at the
prompt or while the model is working. `/thinking last` reprints a block that
already streamed collapsed. Start hidden with `--no-thinking`, or set
`show_thinking = false` in the config.

### Interrupting

**Esc** cancels the turn in flight — mid-generation, not just between steps —
and the session carries on with its history intact. Ctrl+C does the same, and
twice in quick succession force-exits.

Keys pressed while the model is working are not lost: anything typed during a
turn is replayed into the next prompt, so typing ahead still works.

### Tool Calls

Each call prints a header, then a result line with shape and timing plus a
short output preview:

```
  ❯ Bash  cargo test
  ⎿ ✓ 42 lines · 8.1s
    running 10 tests
    test ui::tests::truncate_is_utf8_safe ... ok
    … +40 lines
```

Anything needing approval opens a dialog showing the *actual* request — the
full command for Bash, a line diff for Edit, a content preview and byte count
for Write — so a call can be judged without guessing at it:

```
╭─ ✎  Edit ──────────────────────────────────────────────────╮
│ ~/opt/mycli/bin/parser.py                                  │
│                                                            │
│ -     assert evaluate('-(2+3) * -(4-1)') == -15.0          │
│ +     assert evaluate('-(2+3) * -(4-1)') == 15.0           │
│       print('ok')                                          │
│                                                            │
│ modifies files · approval required                         │
╰────────────────────────────────────────────────────────────╯
   Yes   Yes, don't ask again   No    ←→ move · enter confirm · esc deny
```

The dialog border is colour-coded by risk: cyan for read-only, yellow for
writes and command execution, red for destructive operations.

### Markdown Output

Assistant text is rendered as markdown, and tables are drawn directly rather
than by termimad — which frames a table only when the source is written its own
way, and never insets cells:

```
╭──────┬──────┬─────┬───╮
│ Step │    a │   b │ q │
├──────┼──────┼─────┼───┤
│ 1    │ 1914 │ 899 │ 2 │
│ 2    │  899 │ 116 │ 7 │
╰──────┴──────┴─────┴───╯
```

Column alignment (`:---`, `---:`, `:---:`) is honoured, cells carry inline
markdown, and a table too wide for the terminal shrinks its widest column
rather than wrapping.

Text streams as it arrives, but a construct whose layout depends on lines that
have not arrived yet — a table, a fenced code block — is held until complete.
Rendering a table row at a time is what produced unaligned columns before.

### Status Bar

Two lines pinned to the bottom of the terminal, outside the scroll region:

```
/opt/mycli (main)
↑2.8k ↓1.1k · ctx 4.2%/128k · code · think:on          (omlx) mlx-community_Qwen3.8-27B-mxfp8
```

- **line 1** — working directory and git branch
- **↑/↓** — cumulative input and output tokens for the session
- **ctx** — context window fill from the last turn's input tokens (green/yellow/red)
- **think** — whether reasoning is being displayed
- Token counters reset on model/provider switch

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

## MCP (Model Context Protocol)

MyCLI connects to MCP servers over stdio transport. Tools are auto-discovered at
startup when using the `full` tool tier — add `[[mcp]]` blocks to your config (see
[Configuration](#configuration)), then use `/mcp` in the REPL to see server status.

---

## Benchmarking

MyCLI includes a model benchmark suite for comparing local LLM capabilities across personas and tasks. See [`bench/README.md`](bench/README.md) for details.

```bash
cd bench
./bench.sh                                                            # run all oMLX models (bench.toml, 12 tests)
./bench.sh WhiteRabbit                                                # filter by model name
BENCH_FILE=bench_v2.toml ./bench.sh                                   # enhanced suite (45 tests, all 6 personas)
BENCH_FILE=bench_v2.toml ./bench.sh mlx-community_Qwen3.8-27B-mxfp8   # enhanced suite, specific model
./grade.sh                                                            # auto-grade results via DeepSeek API
```

**Test suites:**
- `bench.toml` — Original 12 tests across 4 personas
- `bench_v2.toml` — Enhanced 45 tests: code (9), math (9), agentic (8), reasoning (7), blueteam (5), redteam (3), data (2), meta (2)

### Refusal comparison

`bench.sh` measures whether a model *can* do a task; [`refusal_test.py`](bench/refusal_test.py)
measures whether it *will* — 8 HTB/OSCP probes across two or more models, scored on refusal,
code blocks actually produced, and ethics boilerplate. Details in
[`bench/README.md`](bench/README.md#refusal-comparison-refusal_testpy).

```bash
cd bench && ./refusal_test.py --open
```

[![Refusal report](bench/refusal-report.png)](bench/refusal_report.example.md)

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

MyCLI is built on top of the **[Cersei SDK](https://github.com/pacifio/cersei)** by
[Adib Mohsin](https://github.com/pacifio) — the agent loop, tool execution, provider
abstraction, memory system and MCP client. Without this SDK, MyCLI would not exist. Thank you.

Enhancements contributed back during MyCLI development: provider tool-call streaming,
message round-trips and thinking mode; Edit fuzzy/line-range matching; MCP JSON-RPC 2.0
notification compliance.

---

## License

MIT
