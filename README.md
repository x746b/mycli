# MyCLI

Lightweight AI coding CLI for testing LLM capabilities — especially local models running on [oMLX](https://github.com/jundot/omlx). Cloud providers (Kimi, DeepSeek, Gemini, OpenAI) supported as first-class fallback.

```bash
$ mycli
                    _____ _     __
                  / ____| |   /_ |
  _ __ ___  _   _| |    | |    | |
 | '_ ` _ \| | | | |    | |    | |
 | | | | | | |_| | |____| |____| |
 |_| |_| |_|\__, |\_____|______|_|
             __/ |
            |___/           v1.1.0

  tools [medium]: Read, Write, Bash, Edit, Glob, Grep, WebSearch
  omlx · Qwen3.8-27B · tools:medium · max_turns:30 · /opt/mycli
  ctrl+c interrupt · ctrl+d exit · / commands · ctrl+o thinking · ctrl+u clear input

───────────────────────────────────────────────────────────────────────────────────────────────
 › hey
───────────────────────────────────────────────────────────────────────────────────────────────

  ✻ Thinking
  │ The user said "hey". Just a greeting, so I'll respond normally.

● Hey! What's up?


/opt/mycli (main)
↑2.9k ↓29 · pp 2381 t/s · tg 153 t/s · ctx 1.1%/262.1k · code · think:on     (omlx) Qwen3.8-27B

───────────────────────────────────────────────────────────────────────────────────────────────
 › /cloud
───────────────────────────────────────────────────────────────────────────────────────────────
  Select cloud: (↑↓ select, Enter confirm, Esc cancel)
    deepseek
    gemini
    kimi
  ▸ openai

 Select reasoning level for gpt-5.6-luna: (↑↓ select, Enter confirm, Esc cancel)
    Default — use the model's default (active)
    Off — disable reasoning
  ▸ Low — faster responses, lighter reasoning
    Medium — balanced speed and depth
    High — deeper reasoning
    Extra high — more reasoning for complex tasks
    Max — highest effort, more token usage

───────────────────────────────────────────────────────────────────────────────────────────────
 › /tools
───────────────────────────────────────────────────────────────────────────────────────────────
  Select tool tier: (↑↓ select, Enter confirm, Esc cancel)
    simple
    medium
  ▸ full (active)

 ───────────────────────────────────────────────────────────────────────────────────────────────
 › /tools
 ───────────────────────────────────────────────────────────────────────────────────────────────
  tools [full]: Read, Write, Bash, Edit, Glob, Grep, WebFetch, Skill, WebSearch,
  mcp command-vault: 19 tools, mcp cve-lookup: 8 tools

Switched to Jundot_Qwen3.8-Flash-Next-oQ4e-mtp (omlx)
```


```bash
# single-shot with tiny model and simple toolset:
mycli -t simple -m RedSage-Qwen3-8B-DPO         

# offensive security persona with full toolset support and bigger model
mycli -p redteam -t full -m orcarouter_Qwen3.8-27B-Uncensored-8B "cybersec prompt"    
```

** ~ 5MB static binary** | **Rust** | **32 tools** | **3 tool tiers** | **6 personas** | **MCP support** | **Hot-swappable models & providers**

---

## Why

Small local LLMs (7B–30B) can chat well but struggle with structured tool calling — wrong JSON, hallucinated tool names, broken edit strings. Larger cloud models handle it effortlessly. MyCLI allows testing and comparing them across the spectrum by:

- Adjusting tool complexity to match model capability (`simple` / `medium` / `full`)
- Hot-switching between local and cloud models mid-conversation
- Tolerating the edit mistakes small models make, instead of failing the edit
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
# show_thinking = false    # start with reasoning off (see Reasoning)

# ─── MCP servers ───────────────────────────────────────────
# Tools auto-discovered on startup (full tier only)
# Codex syntax

[mcp_servers.command-vault]
command = "/opt/command-vault-mcp/.venv/bin/python"
args = ["-m", "command_vault.server"]

[mcp_servers.command-vault.env]
VAULT_DB = "/home/xtk/.local/share/command-vault/vault.db"

[mcp_servers.cve-lookup]
command = "/opt/cve-lookup/.venv/bin/cve-lookup"
args = ["serve"]

[mcp_servers.cve-lookup.env]
NVD_API_KEY = "..."

# ─── Cloud models ──────────────────────────────────────────
[cloud.kimi]
api_key = "sk-..."
model = "kimi-k3"
context_window = 1048576

[cloud.deepseek]
api_key = "sk-..."
model = "deepseek-v4-pro"
context_window = 1048576

[cloud.gemini]
api_key = "..."
model = "gemini-3.1-pro-preview"
context_window = 1048576

[cloud.openai]
api_key = "sk-proj-...." 
admin_key = "sk-admin-..."
model = "gpt-6-astra"
context_window = 1050000
credits = 50.00                 # initial credit balance
credits_since = "2026-08-01"    # the date that figure was true

```

For `kimi-think`, `deepseek`, `deepseek-think` and `gemini`; add
`max_tokens` to any profile to override. `model` and `max_tokens` override the
built-in preset defaults — that is how you run a newer model than the preset ships with.

Environment variables (`MYCLI_MODEL`, `MYCLI_API_KEY`, `MOONSHOT_API_KEY`, `DEEPSEEK_API_KEY`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `OPENAI_ADMIN_KEY`) are also supported.

---

## Usage

### oMLX backend — local LLM inference

```bash
omlx serve --model-dir ~/models --paged-ssd-cache-dir ~/.omlx/cache --port 8000
```

### REPL / single-shot

```bash
mycli                                          # auto-detect local oMLX model
mycli -m Qwen3.8-Flash-Next                    # specific local model
mycli --cloud kimi                             # start with cloud Kimi
mycli -t simple                                # minimal tools for small models
mycli "find the error in ./test.rs and fix it" # single-shot
mycli --cloud deepseek -y "refactor main.rs"   # auto-approve tools
```

### CLI flags

| Flag | Description |
|------|-------------|
| `-m, --model` | Model name (oMLX model ID or cloud model) |
| `--cloud <name>` | Use cloud provider (kimi, deepseek, gemini, openai, or config profile) |
| `--reasoning <level>` | Cloud reasoning effort; `default` uses the model default |
| `-t, --tools <tier>` | Tool tier: `simple`, `medium`, `full`, or `auto` (default) |
| `-p, --persona <name>` | Persona: `code` (default), `redteam`, `blueteam`, `data`, `math`, `agentic` |
| `-y, --yes` | Auto-approve all tool permissions |
| `--no-thinking` | Start with reasoning off — at the model level where the server supports it |
| `--max-turns` | Max agent turns per prompt (default: 30) |
| `-C, --directory` | Working directory |
| `--show-config` | Print resolved config and exit |
| `--version` | Print version and exit |

---

## REPL Commands

| Command | Description |
|---------|-------------|
| `/help` | Show all commands |
| `/model` | Interactive local model picker (switches back from cloud automatically) |
| `/model <name>` | Switch to a local oMLX model |
| `/cloud` | Pick a cloud provider, then reasoning effort for its configured model |
| `/cloud <name>` | Switch to cloud (e.g. `kimi`, `deepseek`, `gemini`) |
| `/reasoning [level]` | Pick or set reasoning effort without resetting the conversation; `default` resets the override |
| `/tools` | Interactive tool tier picker |
| `/tools <tier>` | Switch tier (`simple` / `medium` / `full`) |
| `/persona` | Interactive persona picker |
| `/usage` | Show cloud balances / spend (Kimi, DeepSeek, OpenAI) |
| `/mcp` | Show each MCP server's status — tools discovered, or why it failed |
| `/mcp verbose` | List every discovered MCP tool, grouped by server |
| `/thinking [on\|off]` | Turn reasoning on or off **at the model level** (see below) |
| `/thinking last` | Reprint the last reasoning block |
| `/clear` | Clear screen |
| `/exit` | Exit |

Cloud reasoning is independent of reasoning **display**: `Ctrl+O` and cloud
`/thinking` hide/show reasoning, while `/reasoning high` changes the model's
actual effort. The footer shows `effort:high` alongside `think:on/off`.
The picker offers only known supported levels for the configured model;
unknown models keep their server default. Esc cancels without switching.

Selections are remembered per profile for the current session. For a persistent
default, add `reasoning_effort = "high"` under `[cloud.openai]` (or another cloud
profile) in your config. `mycli --cloud openai --reasoning high` overrides it.

You can use a single DeepSeek profile and select Off, Low, High, or Max; separate
`deepseek-think` profiles are optional. Existing profiles still work. Kimi K3
offers Low, High, and Max, while Gemini choices depend on the model.

Modern OpenAI models use the Responses API so reasoning works with function
tools. Requests use `store: false` and replay encrypted reasoning state between
tool calls. Custom OpenAI proxies must support `/responses` for these models.
Supported effort choices follow the [OpenAI model documentation](https://developers.openai.com/api/docs/models/gpt-5.6-luna),
[DeepSeek thinking controls](https://api-docs.deepseek.com/guides/thinking_mode/),
[Kimi K3 model usage](https://github.com/MoonshotAI/Kimi-K3#6-model-usage), and
[Gemini compatibility documentation](https://ai.google.dev/gemini-api/docs/openai#thinking).

### Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+U` | Clear the entire current input, including multiline pastes, from any cursor position |
| `Ctrl+Y` | Restore the text cleared with Ctrl+U |
| `Esc` | Interrupt the running turn, at any point during generation |
| `Ctrl+O` | Show/hide reasoning **display** (works at the prompt *and* mid-turn) |
| `Ctrl+C` | Interrupt the current turn (twice in quick succession to force exit) |
| `Ctrl+D` | Exit |
| `Tab` | Complete slash commands |
| `←` `→` / `Tab` | Move between options in an approval dialog |
| `Enter` / `Esc` | Confirm / deny an approval dialog |

All pickers use arrow keys, Enter to confirm, Esc to cancel. All switches are hot — model, provider, tool tier, and persona can change mid-session without restarting.

---

## Tool Tiers

Designed to match tool complexity to model capability:

| Tier | Tools | Best for |
|------|-------|----------|
| **simple** | Read, Write, Bash | small models — minimal surface, hard to mess up |
| **medium** | + Edit, Glob, Grep, WebSearch | 24B+ models — structured tools, edit tolerance helps |
| **full** | + WebFetch, Skill, MCP tools | bigger local and cloud models — full power |

**Auto-detection:** local providers default to `medium`, cloud defaults to `full`.

The system prompt adapts to the tier — small models only see descriptions of tools they actually have access to. MCP servers start on `full` only.

---

## Features

### Personas

| Persona | Focus |
|---------|-------|
| **code** | General coding assistant (default) |
| **redteam** | Offensive security — pentesting, exploitation, CTF, no guardrails |
| **blueteam** | Defensive security — threat detection, forensics, SIEM/YARA/Sigma rules |
| **data** | Data processing — parse, transform, analyze any format |
| **math** | Mathematics and cryptography — number theory, modular arithmetic, RSA/ECC/AES |
| **agentic** | Strict instruction following — tool use, structured output, format adherence |

Switch with `/persona`, `-p`, or `persona = "redteam"` in config.

### Reasoning

Models that emit reasoning have it streamed inline, dimmed behind a gutter:

```
  ✻ Thinking
  │ Recursive-descent parser. Tokenizer first, then one function per
  │ precedence level, so `*` binds before `+`.
```

Reasoning effort and its display are separate controls:

- **`/thinking on|off`** switches reasoning at the **model level**. Local servers
  that render a chat template (oMLX, vLLM, SGLang) expose the template's own
  thinking flag, so this genuinely stops the model reasoning — saving the tokens
  and the latency, not just hiding the output. For cloud models this command
  changes display only; use **`/reasoning`** to select model-level effort.
- **`Ctrl+O`** only shows or hides what arrives, collapsing it to a live counter
  (`✻ Thinking… 412 chars · ctrl+o to show`). Works at the prompt or mid-turn.

Not every model reasons. A successful request proves nothing — oMLX accepts the
flag for a template that has none and ignores it — so mycli reports what it has
actually observed: `(model level)` once reasoning has been seen, or
`no effect — this model has not produced any reasoning` after a turn that
produced none.

`/thinking last` reprints a block that streamed collapsed. Start off with
`--no-thinking` or `show_thinking = false`.

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

The border is colour-coded by risk: cyan for read-only, yellow for writes and
command execution, red for destructive operations.

### Edit Tolerance

Small local models routinely quote code back with the indentation flattened or
a run of spaces collapsed. A byte-exact `old_string` requirement loses every
such edit, so `Edit` tries a ladder of progressively more tolerant strategies:
exact, line-trimmed, block-anchor (guarded by a similarity floor), whitespace-
normalized, then indentation-flexible. `start_line`/`end_line` remain available
as an alternative to string matching.

Every strategy only ever locates text that really exists in the file, so
tolerance changes *where* a match is found, never *what* gets written — and a
match that appears more than once is still refused rather than guessed at.

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

Column alignment (`:---`, `---:`, `:---:`) is honoured and cells carry inline
markdown. A table too wide for the terminal shrinks columns proportionally and
wraps cells onto extra lines, rather than truncating the column that usually
carries the explanation.

Small models often emit a whole table on one line, sometimes with the
surrounding prose run into it (`...definition.| n | prime ||---|---|| 1 | No |In
short...`). That is detected and split back into rows.

Text streams as it arrives, but a construct whose layout depends on lines that
have not arrived yet — a table, a fenced code block — is held until complete.

### Math

LaTeX is converted to Unicode before rendering — `\(`, `\[`, `$$` and `$` spans
alike:

```
\[ x = \frac{-b \pm \sqrt{b^2-4ac}}{2a} \]   →   x = (-b ± √(b²-4ac))/(2a)
```

Greek letters, relations, big operators, super- and subscripts, roots and
fractions are mapped. An expression that cannot be mapped is left as written
rather than mangled.

Code is never touched — a fenced block or backtick span may legitimately
contain `$` or a backslash. Nor is a lone `$` in prose: `costs $5 to $10` and
`echo $HOME` are left alone, because a `$…$` span has to look like an
expression before it is treated as one.

### Status Bar

Two lines pinned to the bottom of the terminal, outside the scroll region:

```
/opt/mycli (main)
↑2.8k ↓1.1k · pp 626 t/s · tg 95.9 t/s · ctx 4.2%/128k · code · think:on   (omlx) Qwen3.8-27B
```

- **line 1** — working directory and git branch
- **↑/↓** — cumulative input and output tokens for the session
- **pp / tg** — prompt processing and token generation throughput (below)
- **ctx** — context window fill from the last turn's input tokens (green/yellow/red)
- **think** — whether reasoning is on
- Token counters reset on model/provider switch

### Throughput

`pp` (prefill) and `tg` (decode) are measured per user prompt, across every
turn it takes.

Where the provider reports its own per-phase timings, those are used and the
numbers match what the server reports: oMLX returns `prompt_eval_duration`,
`generation_duration` and `prompt_tokens_details.cached_tokens`, so a turn
logged as `958 tokens in 9.89s (104.1 tok/s)` shows as `tg 104 t/s`.

Without those fields the rates are measured client-side. A turn splits cleanly
in two — nothing comes back until the prompt has been processed — so the wait
for the first token is prefill and everything after it is generation. Tool
execution happens after the turn completes, so it never lands inside either
window.

### Context Window

Taken from the first of these that answers:

1. `context_window` in the config — on a cloud profile, or top-level for the default provider.
2. What a local server states: oMLX reports `max_model_len` on `/v1/models`.
3. A guess from the model name.

The guess is coarse — an unrecognised id falls back to 32,768, and a 400k model
treated as 32k shows a full context bar and compacts far too early. Set it
explicitly for any cloud model it does not recognise:

```toml
[cloud.openai]
model = "gpt-5.6-luna"
context_window = 400000   # tokens the model can hold
# max_tokens is a different setting: the cap on a single response
```

A profile's window applies while that profile is active; the top-level setting
describes the default provider and does not follow you onto a cloud one.

At 90% of the window the conversation is compacted: older turns are summarised
and replaced with that summary, keeping the ten most recent messages. The split
never cuts a tool round in half — a `tool_result` whose `tool_use` had just been
summarised away would be rejected — so the boundary moves to the next plain user
message, and compaction is skipped if there isn't one. Three consecutive
failures disable it for the session.

### Web Search

Against a local oMLX server, `WebSearch` calls its `POST /v1/web/search`
endpoint, which uses whichever provider is set in the server's settings — DDGS,
DuckDuckGo, Brave, or SearXNG.

Going through the server means one place to configure search and one place a key
lives, and it needs no key at all on DDGS. It is registered on the `medium` and
`full` tiers, since small local models are the ones least able to answer from
memory. A cloud provider has no such endpoint, so the tool is not offered there;
`WebFetch` covers reading URLs on `full`.

### Safety

- **Permission system** — interactive approval for write/execute operations, or `-y` to auto-approve
- **Cost guard hook** — set `cost_limit` in config to cap cloud API spend per session
- **Tool tiers** — limit what tools the model can access

### Checking the Terminal Output

A raw capture (`script`) records the byte stream, not the screen, so cursor
motion and scroll-region bugs are invisible in it. `tools/vt.py` replays a
capture onto a virtual screen and prints what a terminal would actually show:

```bash
printf '/exit\n' | script -qc "stty rows 30 cols 110; ./target/debug/mycli" /dev/null \
  | python3 tools/vt.py 30 110
```

Each line is numbered, with the cursor position and scroll region reported at the end.

---

## MCP (Model Context Protocol)

MyCLI connects to MCP servers over stdio transport. Tools are auto-discovered at
startup on the `full` tool tier — add `[mcp_servers.<name>]` tables to your config (see
[Configuration](#configuration)), then use `/mcp` in the REPL to see server status.

The table syntax matches [Codex MCP configuration](https://developers.openai.com/codex/mcp).
Copy command-based server definitions directly, including either inline `env`
or a nested environment table:

```toml
[mcp_servers.command-vault]
command = "/opt/command-vault-mcp/.venv/bin/python"
args = ["-m", "command_vault.server"]
enabled = true
# cwd = "/path/to/project"

[mcp_servers.command-vault.env]
VAULT_DB = "/path/to/vault.db"
VAULT_READONLY = "1"
```

Supported fields are `command`, `args`, `env`, `cwd`, and `enabled`. HTTP `url`
servers and other Codex-specific settings are reported as unsupported in
`/mcp`; mycli does not start servers with unsupported settings. This prevents
copied tool filters or approval settings from being silently ignored.

Legacy `[[mcp]]` entries remain supported. If both formats define the same
server in one file, the named table wins. Project definitions replace global
definitions **by server name**, keeping other servers; use `enabled = false`
to disable an inherited server. mycli reads its own config files, so copying
settings does not change or automatically load your Codex configuration.

---

## Benchmarking

A model benchmark suite for comparing local LLM capabilities across personas and tasks. See [`bench/README.md`](bench/README.md) for details.

```bash
cd bench
./bench.sh                                                            # run all oMLX models (bench.toml, 12 tests)
./bench.sh Qwen3.8-Flash                                              # filter by model name
BENCH_FILE=bench_v2.toml ./bench.sh                                   # enhanced suite (45 tests, all 6 personas)
./grade.sh                                                            # auto-grade results via DeepSeek API
```

- `bench.toml` — 12 tests across 4 personas
- `bench_v2.toml` — 45 tests: code (9), math (9), agentic (8), reasoning (7), blueteam (5), redteam (3), data (2), meta (2)

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

MyCLI is built on the [Cersei SDK](https://github.com/pacifio/cersei) — a modular Rust SDK for building coding agents, vendored into this repo.

```
mycli (CLI binary)
  └── cersei SDK
      ├── cersei-types       Provider-agnostic types
      ├── cersei-provider    OpenAI-compatible provider (oMLX, Kimi, DeepSeek, etc.)
      ├── cersei-tools       32 built-in tools, permissions, skills
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
