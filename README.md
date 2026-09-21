# MyCLI

Want a lightweight coding CLI that doubles as a test bench for local and cloud LLMs? Meet MyCLI. Switch models, plug in MCP tools over stdio or HTTP, and explore highlighted code and full tool output—all from your terminal.

Here’s what it looks like:
```text
$ mycli
                    _____ _     __
                  / ____| |   /_ |
  _ __ ___  _   _| |    | |    | |
 | '_ ` _ \| | | | |    | |    | |
 | | | | | | |_| | |____| |____| |
 |_| |_| |_|\__, |\_____|______|_|
             __/ |
            |___/           v2.0.0

  tools [medium]: Read, Write, Bash, Edit, Glob, Grep, WebSearch
  Session: Untitled session (891086a4-eb38-4b43-8270-0a142919db7a)
  omlx · Qwen3.8-27B · tools:medium · max_turns:30 · /opt/mycli
  ctrl+c interrupt · ctrl+d exit · / commands · ctrl+o thinking · ctrl+t tool output · ctrl+u clear input

───────────────────────────────────────────────────────────────────────────────────────────────
 › hey
───────────────────────────────────────────────────────────────────────────────────────────────

  ✻ Thinking
  │ The user said "hey". Just a greeting, so I'll respond normally.

● Hey! What's up?

───────────────────────────────────────────────────────────────────────────────────────────────
/opt/mycli (main)
↑1.9k ↓29 · pp 2381 t/s · tg 153 t/s · ctx 1.1%/262.1k · code · think:on     (omlx) Qwen3.8-27B
```


See what’s available on your local inference server:

oMLX: [Website](https://omlx.ai/)
```text
───────────────────────────────────────────────────────────────────────────────────────────────
 › /model
───────────────────────────────────────────────────────────────────────────────────────────────
  Select model (1/21) · ↑↓ select, Enter confirm, Esc cancel
  ▸ Qwen3.6-35B-A3B-8bit (active)
    mlx-community_Qwen3.8-Flash-Next-Uncensored-oQ5e-mtp
    orcarouter_Qwen3.8-27B-Uncensored-MLX-8-bit
    trend-cybertron_Llama-Primus-Nemotron-70B-Instruct-oQ4e
    DavidAU_Qwen3.8-27B-TWIN-TURBO-Fable-Cold-Fusion-709-L-Uncensored-oQ8-mtp
    nightmedia_gpt-oss-120b-heretic-v2-mxfp4-q8-hi-mlx
...
```

DS4 - [USAGE.md](https://github.com/x746b/ds4/blob/main/USAGE.md)
```text
───────────────────────────────────────────────────────────────────────────────────────────────
 › /model
───────────────────────────────────────────────────────────────────────────────────────────────
    glm-5.3-flash
    glm-5.3-flash-chat
    glm-5.3-flash-reasoner
  ▸ deepseek-v4.1-flash 

───────────────────────────────────────────────────────────────────────────────────────────────
 › /reasoning
───────────────────────────────────────────────────────────────────────────────────────────────
  Select reasoning level for deepseek-v4.1-flash (1/5) · ↑↓ select, Enter confirm, Esc cancel
  ▸ Default — use the model's default (active)
    Off — disable reasoning
    Low — faster responses, lighter reasoning
    High — deeper reasoning
    Max — highest effort, more token usage
```

Switch between local models with custom profiles:
```text
───────────────────────────────────────────────────────────────────────────────────────────────
 › /local
───────────────────────────────────────────────────────────────────────────────────────────────
  Select local (2/9) · ↑↓ select, Enter confirm, Esc cancel
    coldfusion
  ▸ coldfusion-heretic
    ds4
    ds4fast
    ds4think
    gemma4
    glm
    glmfast
    qwen36
```

Switch to a cloud provider and pick a reasoning level:
```text
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
```


MyCLI supports three tool tiers, with MCP available in the `full` tier:
```text
───────────────────────────────────────────────────────────────────────────────────────────────
 › /tools
───────────────────────────────────────────────────────────────────────────────────────────────
  Select tool tier: (↑↓ select, Enter confirm, Esc cancel)
    simple
    medium (active)
  ▸ full 

 tools [full]: Read, Write, Bash, Edit, Glob, Grep, WebFetch, Skill, WebSearch, 
 mcp caido: 66 tools, mcp command-vault: 20 tools, mcp cve-lookup: 8 tools, mcp ida-headless: 66 tools

Switched to mlx-community_Qwen3.8-Flash-Next-oQ5e-mtp (omlx)
```


Single-shot examples:
```bash
# single-shot with tiny model and simple toolset:
mycli -t simple -m RedSage-Qwen3-8B-DPO "Explain Rust ownership briefly."

# offensive security persona with full toolset support and bigger model
mycli -p redteam -t full -m orcarouter_Qwen3.8-27B-Uncensored "cybersec prompt"    
```

**Native binary** | **Rust** | **32 tools** | **3 tool tiers** | **Editable personas** | **MCP support** | **Cybersecurity benchmarks** | **Hot-swappable models & providers**

---

## Why

Small local LLMs (7B–30B) can chat well but struggle with structured tool calling — wrong JSON, hallucinated tool names, broken edit strings. Larger cloud models tend to handle it more reliably. MyCLI allows testing and comparing them across the spectrum by:

- Adjusting tool complexity to match model capability (`simple` / `medium` / `full`)
- Hot-switching between local and cloud models mid-conversation
- Tolerating the edit mistakes small models make, instead of failing the edit
- Keeping the system prompt lean and tier-appropriate — small models only see tools they can use
- Running focused capability and refusal benchmarks, then grading full responses with a configured cloud model or ChatGPT-authenticated Codex

---

## Install

```bash
git clone https://github.com/x746b/mycli && cd mycli
cargo build --release
```
Requires Rust 1.85+, OpenSSL dev libraries (`libssl-dev` / `openssl-devel`).

---

## Configuration

Config lives in `~/.config/mycli/config.toml` (global) and `.config/mycli/config.toml` (project-level).
`XDG_CONFIG_HOME` overrides `~/.config` on Linux and macOS. Each file falls back to
its legacy `~/.mycli/` or project `.mycli/` location only when the new file is absent.
History is saved in the new global directory, with legacy history read on first use.

```toml
# ─── Local (oMLX) ──────────────────────────────────────────
api_key = "your-omlx-key"
# base_url defaults to http://127.0.0.1:8000/v1
# model = "mlx-community_Qwen3.8-27B-mxfp8"   # empty/unset = auto-detect first loaded

# ─── Persona & tool tier ───────────────────────────────────
# persona = "code"         # any persona in system-prompts.toml, including neutral
# tool_tier = "auto"       # auto = medium for local, full for cloud
# cost_limit = 1.0         # stop agent after $1 cloud spend (0 = unlimited)
# show_thinking = false    # start with reasoning off (see Reasoning)

# ─── Named local profiles ──────────────────────────────────
# Select with /local, /local ds4think, or mycli --local ds4think.
[local.glm]
model = "glm-5.2"
temperature = 0.6
tool_tier = "medium"
web_search = true          # oMLX-specific extension

[local.ds4think]
base_url = "http://127.0.0.1:8000/v1"
api_key = ""              # set your server key if required
model = "deepseek-reasoner"
max_tokens = 8192
max_turns = 30
context_window = 32768
reasoning_effort = "max"
tool_tier = "full"
show_thinking = true

# ─── MCP servers ───────────────────────────────────────────
# Tools auto-discovered on startup (full tier only)

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

For `kimi-think`, `deepseek`, `deepseek-think`, and `gemini`, add
`max_tokens` to a profile to override its output limit. `model` and `max_tokens` override the
built-in preset defaults — that is how you run a newer model than the preset ships with.

Environment variables (`MYCLI_MODEL`, `MYCLI_API_KEY`, `MOONSHOT_API_KEY`, `DEEPSEEK_API_KEY`, `GEMINI_API_KEY`, `OPENAI_API_KEY`, `OPENAI_ADMIN_KEY`) are also supported.

`/local` loads named local profiles from config and rereads them on each switch.
Profiles can use different servers, including LAN addresses and SSH tunnels.
To move DS4 profiles out of `/cloud`, rename `[cloud.ds4]`, `[cloud.ds4fast]`,
and `[cloud.ds4think]` to `[local.ds4]`, `[local.ds4fast]`, and
`[local.ds4think]`; their existing model, endpoint, key, token limits, context
window, and reasoning settings work there.

A local profile inherits the top-level endpoint when `base_url` is omitted.
It inherits the top-level key only for that same endpoint; `api_key = ""`
explicitly selects no authentication. An empty `model` auto-detects the first
model on that server. 

Omitted `max_tokens`, `max_turns`, `temperature`, `top_p`, `min_p`, `thinking`, `tool_tier`, `persona`, and `show_thinking` use top-level defaults. 
Omitted `context_window` (or `0`) uses detection, and omitted `reasoning_effort` uses the model's default. `temperature` accepts 0–2, subject to server support.

`web_search` defaults to false for named profiles because it requires oMLX's
search endpoint. Settings from the previous profile are cleared when switching.

`/reasoning` also works for local profiles whose model has known reasoning
levels, including the DeepSeek aliases. Selecting `/local <name>` again
reloads its saved settings, including reasoning effort.

Project profiles replace global definitions with the same name. Explicit CLI
flags override the selected profile, and `--local` conflicts with `--cloud`.
See [config.example.toml](config.example.toml) for all options.

For servers that support detailed sampling, local profiles also accept
`top_p` and `min_p` (0–1). Omitted sampling settings are not sent unless a
top-level default is configured. `thinking = false` explicitly disables
model-level reasoning through `chat_template_kwargs.enable_thinking`;
`show_thinking` controls display (and retains the legacy off-at-start behavior
when `thinking` is unset). An explicit `thinking` value overrides the effort's
on/off toggle. `--no-thinking` overrides the profile to off.

For example, the DS4 chat-completions endpoint supports this non-thinking
profile with an explicit response cap:

```toml
[local.glmfast]
base_url = "http://127.0.0.1:8000/v1"
model = "glm-5.3-flash-nothink"
temperature = 1.0
top_p = 0.95
min_p = 0.05
thinking = false
max_tokens = 2048
```

`--mtp` and `--ctx` belong to DS4 startup. mycli's `context_window` describes
the server's capacity; it does not change the server's allocated context.

---

## Usage with the oMLX backend

### Local LLM inference

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
| `--local <name>` | Load a named `[local.<name>]` profile |
| `--reasoning <level>` | Model reasoning effort; `default` uses the server default |
| `-t, --tools <tier>` | Tool tier: `simple`, `medium`, `full`, or `auto` (default) |
| `--resume [id/name]` | Resume saved context; omitted selector uses the latest session |
| `-p, --persona <name>` | Persona from `system-prompts.toml`; `code` by default, `neutral` for empty persona text |
| `-y, --yes` | Auto-approve all tool permissions |
| `--no-thinking` | Start with reasoning off — at the model level where the server supports it |
| `--max-turns` | Max agent turns per prompt (default: 30) |
| `-C, --directory` | Working directory |
| `--show-config` | Print resolved config with API keys and sensitive MCP environment values redacted, then exit |
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
| `/local [name]` | Pick or load a local profile with its configured endpoint and model settings |
| `/reasoning [level]` | Pick or set reasoning effort without resetting the conversation; `default` resets the override |
| `/tools` | Interactive tool tier picker |
| `/tools <tier>` | Switch tier (`simple` / `medium` / `full`) |
| `/persona [name]` | Pick or switch to any configured persona |
| `/skill [name args]` | Pick or run a skill; `list`, `paths`, and `reload` inspect/manage the catalog |
| `/prompts path` | Show active prompt source and preferred file |
| `/prompts reload` | Reload prompt catalog and reset conversation |
| `/usage` | Show cloud balances / spend (Kimi, DeepSeek, OpenAI) |
| `/mcp` | Show each MCP server's status — tools discovered, or why it failed |
| `/mcp verbose` | List every discovered MCP tool, grouped by server |
| `/bench` | Open the benchmark menu: capability tests, refusal comparison, grading, and local-model listing |
| `/grade` | Select and grade saved benchmark results with a configured cloud provider or Codex |
| `/thinking [on\|off]` | Turn reasoning on or off **at the model level** (see below) |
| `/thinking last` | Reprint the last reasoning block |
| `/sessions` | Pick a saved session; `list`, `path`, or `rename [id] <name>` |
| `/resume [id/name]` | Resume by ID/name, or open the session picker |
| `/memory` | Show global memory; `path`, `topics`, or `reload` |
| `/remember <fact>` | Append a durable fact to global `MEMORY.md` |
| `/compact [focus]` | Summarize older conversation; optional focus instructions |
| `/compact status` | Show estimated input usage and resolved context budget |
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

### Local reasoning levels

Cold-Fusion supports `/reasoning low|medium|xhigh|einstein|spoon` out of the box.
To declare or override levels, add a profile in `~/.config/mycli/config.toml`:

```toml
[local.coldfusion]
model = "your-exact-Cold-Fusion-model-id"
reasoning_levels = ["low", "medium", "xhigh", "einstein", "spoon"]
reasoning_effort = "default"
```

Use `/local coldfusion`, then `/reasoning` to pick or `/reasoning spoon` to select.
**`/local` loads profile settings; `/model` selects a model directly.**
`/reasoning default` restores the server default. Omit `reasoning_levels` to use
built-in choices, or set `[]` to disable explicit effort levels.

Only declare levels supported by the backend/template. Standard Qwen3.6 and Gemma4
templates expose thinking on/off instead; use `/thinking on` or `/thinking off`.
See [profile examples](config.example.toml). Existing `{REASON:...}` tags may override
request settings, so compare controls in a fresh conversation.

### Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+U` | Clear the entire current input, including multiline pastes, from any cursor position |
| `Ctrl+Y` | Restore the text cleared with Ctrl+U |
| `Esc` | Interrupt the running turn, at any point during generation |
| `Ctrl+O` | Show/hide reasoning **display** (works at the prompt *and* mid-turn) |
| `Ctrl+T` | Browse full tool output; queues the viewer while a turn runs |
| `Ctrl+C` | Interrupt the current turn (twice in quick succession to force exit) |
| `Ctrl+D` | Exit |
| `Tab` | Complete slash commands |
| `←` `→` / `Tab` | Move between options in an approval dialog |
| `Enter` / `Esc` | Confirm / deny an approval dialog |

All pickers use arrow keys to navigate, Enter to confirm, and Esc to cancel. All switches are hot — model, provider, tool tier, and persona can change mid-session without restarting.

During generation, press **Ctrl+C once** or **Esc** to cancel the request,
then enter your correction at the next prompt. mycli closes the HTTP stream,
including while waiting for the first token or during silent reasoning.
oMLX and DS4 can use that disconnect to abort generation; an in-flight GPU
operation may finish before the server becomes idle. The correction starts
a new request with the conversation so far. The interrupted response is not
added to model history, and actions from earlier tool calls are not undone.
This cancels generation entirely; it does not force the current response to
finish its reasoning and immediately produce an answer. Ctrl+O only toggles
the reasoning display.

---

## Tool Tiers

Designed to match tool complexity to model capability:

| Tier | Tools | Best for |
|------|-------|----------|
| **simple** | Read, Write, Bash | small models — minimal surface, hard to mess up |
| **medium** | + Edit, Glob, Grep, WebSearch | 24B+ models — structured tools, edit tolerance helps |
| **full** | + WebFetch, Skill, MCP tools | bigger local and cloud models — full power |

**Auto-detection:** local providers default to `medium`; cloud providers default to `full`.

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
| **neutral** | Empty persona text, no house style; tool guidance and runtime context remain |

Switch with `/persona`, `-p`, or `persona = "code"` in config. Names are case-insensitive.

Prompts are loaded from `$MYCLI_PROMPTS`, then `~/.config/mycli/system-prompts.toml`
(or `$XDG_CONFIG_HOME/mycli/system-prompts.toml`), then legacy
`~/.mycli/system-prompts.toml`, then embedded defaults. An explicit path is authoritative;
a missing or invalid explicit file warns and uses embedded defaults at startup.
Invalid reloads leave the active catalog and conversation unchanged.

Copy the bundled `system-prompts.toml` into the global config directory to edit it.
Add or delete `[personas.<name>]` tables, then run `/prompts reload`; the picker updates
without recompiling. The external catalog replaces all default personas. `neutral` is
always available and must have `prompt = ""`. An unknown or removed active persona
falls back to `code`, or `neutral` when `code` is absent.

Each persona has a `prompt` and optional `description`. Tier `guidelines` and `style`
are editable; omitted tiers inherit embedded defaults. Tool registration remains in
code, so prompt edits do not grant tools. Neutral always suppresses tier style;
`[style].suppress_for_personas` adds other names, and `suppress_for_models` accepts
case-insensitive globs matched against the resolved model ID. The bundled catalog
suppresses style for `*cold-fusion*`. An external catalog's omitted `[style]` has no
additional suppression rules. Environment, memory, project instructions, and optional
WebSearch guidance still form part of the system prompt.

A successful reload resets the conversation, just like switching personas. Failed parsing
or agent rebuilding keeps the previous active prompts. Existing persona wording is
preserved; see [the modernization proposal](docs/persona-modernization.md) for an
optional replacement catalog and evaluation plan.

### Reasoning

Models that emit reasoning have it streamed inline, dimmed behind a gutter:

```text
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
and the session carries on with its history intact. Ctrl+C does the same; pressing
it twice in quick succession forces an exit.

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

```text
╭─ ✎  Write ───────────────────────────────────────────────────────────────────────────────────╮
│ /tmp/dijkstra.rs                                                                             │
│ 44 lines · 1280 bytes                                                                        │
│                                                                                              │
│ + use std::cmp::Reverse;                                                                     │
│ + use std::collections::{BinaryHeap, HashMap};                                               │
│ +                                                                                            │
│ + type Graph = HashMap<usize, Vec<(usize, i64)>>;                                            │
│ +                                                                                            │
│ + pub fn dijkstra(graph: &Graph, start: usize) -> HashMap<usize, i64> {                      │
│ +     let mut dist: HashMap<usize, i64> = HashMap::new();                                    │
│ +     let mut heap = BinaryHeap::new();                                                      │
│ +                                                                                            │
│ +     dist.insert(start, 0);                                                                 │
│ +     heap.push(Reverse((0i64, start)));                                                     │
│ +                                                                                            │
│   … 32 more lines                                                                            │
│                                                                                              │
│ modifies files · approval required                                                           │
╰──────────────────────────────────────────────────────────────────────────────────────────────╯
  Yes   Yes, don't ask again   No    ←→ move · enter confirm · esc deny
  ...
  ✓ allowed

╭─ ✎  Edit ────────────────────────────────────────────────────────────────────────────────────╮
│ /tmp/dijkstra.rs                                                                             │
│                                                                                              │
│ -     let w_dest = rows.iter().map(|r| r[0].len()).max().unwrap();                           │
│ -     let w_dist = rows.iter().map(|r| r[1].len()).max().unwrap();                           │
│ +     let w_dest = rows.iter().map(|r| r.0.len()).max().unwrap();                            │
│ +     let w_dist = rows.iter().map(|r| r.1.len()).max().unwrap();                            │
│                                                                                              │
│ -     let total_w = 2 + w_dest + 2 + w_dist + 2 + rows.iter().map(|r| r[2].len()).max().unw… │
│ +     let total_w = 2 + w_dest + 2 + w_dist + 2 + rows.iter().map(|r| r.2.len()).max().unwr… │
│                                                                                              │
│ modifies files · approval required                                                           │
╰──────────────────────────────────────────────────────────────────────────────────────────────╯
    Yes   Yes, don't ask again   No    ←→ move · enter confirm · esc deny

╭─ ❯  Bash ────────────────────────────────────────────────────────────────────────────────────╮
│ cd /tmp && rustc dijkstra.rs -o dijkstra && ./dijkstra                                       │
│                                                                                              │
│ runs a command · approval required                                                           │
╰──────────────────────────────────────────────────────────────────────────────────────────────╯
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

```text
╭─────────────┬─────────┬────────────────────────────────────────────────────────╮
│ Destination │    Cost │ Route                                                  │
├─────────────┼─────────┼────────────────────────────────────────────────────────┤
│ Madrid      │  625 km │ Lisbon ▸ Madrid                                        │
├─────────────┼─────────┼────────────────────────────────────────────────────────┤
│ Barcelona   │ 1245 km │ Lisbon ▸ Madrid ▸ Barcelona                            │
├─────────────┼─────────┼────────────────────────────────────────────────────────┤
│ Paris       │ 1675 km │ Lisbon ▸ Madrid ▸ Paris                                │
├─────────────┼─────────┼────────────────────────────────────────────────────────┤
│ Lyon        │ 1885 km │ Lisbon ▸ Madrid ▸ Barcelona ▸ Lyon                     │
├─────────────┼─────────┼────────────────────────────────────────────────────────┤
│ Marseille   │ 1750 km │ Lisbon ▸ Madrid ▸ Barcelona ▸ Marseille                │
├─────────────┼─────────┼────────────────────────────────────────────────────────┤
│ Milan       │ 2270 km │ Lisbon ▸ Madrid ▸ Barcelona ▸ Marseille ▸ Milan        │
├─────────────┼─────────┼────────────────────────────────────────────────────────┤
│ Rome        │ 2845 km │ Lisbon ▸ Madrid ▸ Barcelona ▸ Marseille ▸ Milan ▸ Rome │
╰─────────────┴─────────┴────────────────────────────────────────────────────────╯
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

```text
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

Window precedence: profile/top-level `context_window`, local server metadata
(`/v1/models`, including oMLX's `max_model_len`), then a conservative model-name
fallback. A profile's window does not follow you onto another provider. Set an
explicit limit when a proxy or local deployment uses a smaller window than the model:

```toml
[local.custom]
model = "your-model-id"
context_window = 32768
max_tokens = 4096  # response cap; reserved separately from input space
```

Automatic compaction checks before each request, including subsequent user prompts,
at 90% of the input budget after reserving output and framing space. It summarizes
older context in bounded chunks and preserves recent turns and tool-call/result pairs.
Provider token counts calibrate usage; counts between requests remain estimates.

Use `/compact` to run it manually, `/compact preserve the API decisions` to focus
the summary, or `/compact status` to inspect the budget. Esc/Ctrl+C cancels.
Failed, empty, truncated, or non-shrinking summaries leave history intact. Three
failed automatic attempts pause auto-compaction; successful manual compaction resets
that state. See [context management and upstream comparison](docs/context-management.md).

```text
──────────────────────────────────────────────────────────────────────────────────────────────────────────────
 › /compact status
──────────────────────────────────────────────────────────────────────────────────────────────────────────────
  Context: ~14924 input tokens / 262144 window (244736 usable for input); 42 messages. Auto-compaction: ready.

──────────────────────────────────────────────────────────────────────────────────────────────────────────────
 › /compact
──────────────────────────────────────────────────────────────────────────────────────────────────────────────
  Compacting older context… Esc or Ctrl+C cancels; original history is retained on failure.
  Compacted 42 → 6 messages; ~7907 tokens freed. Conversation preserved
```

Compaction updates the active context checkpoint while retaining earlier messages in the session's append-only transcript. See session storage below.


### Sessions and global memory

Sessions are saved automatically under `~/.config/mycli/sessions/<id>/`:
`transcript.jsonl` archives conversation events; `context.json` holds the latest
resumable context. Compaction keeps the archive. `XDG_CONFIG_HOME` is supported.

```text
/sessions rename Rust harness work
/sessions
/resume <id-or-name>
/memory
/remember Prefer concise status updates.
```

`/quit` (also `/exit` and `/q`) asks **Keep this session? [Y/n]**. Y or Enter keeps
both files; N deletes only that session folder. Ctrl+C and EOF keep saved sessions
without asking. During generation, Ctrl+C first interrupts the request. Single-shot
runs are kept automatically. Names are labels; IDs and folder paths stay unchanged.

Global notes live in `~/.config/mycli/MEMORY.md`, with optional topic files in
`memory/`. A bounded index is loaded into context; `/memory reload` applies external
edits without resetting conversation. `/remember` saves and applies a fact directly.
Deleting a session leaves global memory and typed-input `history` intact. Resume
uses current configured credentials and serving limits. See [session details](docs/sessions.md).

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

## Skills

Use `/skill` for a picker, `/skill list` for a listing, or `/skill debug failing tests`
to run a skill directly. These commands work on every tool tier; the model-callable
`Skill` tool remains on `full`. Both use the same catalog and normal tool permissions.

Internal skills (`simplify`, `remember`, `debug`, `stuck`, `verify`, `commit`, `loop`)
live in `~/.config/mycli/skills-internal.toml` (`XDG_CONFIG_HOME` supported). Copy the
bundled [catalog](skills-internal.toml) there to edit, add, disable, or remove skills.
`MYCLI_SKILLS` overrides its location; missing files use embedded defaults.
`/skill reload` applies valid edits without resetting conversation; invalid edits
leave the active catalog intact. `/skill paths` shows the active file and search roots.

External discovery checks `.config/mycli/skills`, `.claude/commands`, `.claude/skills`,
and `.agents/skills` in the project and user directories, plus `skill_paths` from
config. Nested commands and `SKILL.md` folders are supported. Internal names win,
then project, user, and configured paths. Use `$ARGUMENTS` in templates; external
skills also receive their base directory for references and scripts.

Here’s an example from my setup—internal skills alongside my own Claude-compatible skills:

```text
───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
 › /skill
───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  Select skill (Enter runs, Esc cancels) (1/55) · ↑↓ select, Enter confirm, Esc cancel
  ▸ commit [internal] — Create a git commit with a well-crafted message.
    debug [internal] — Investigate and diagnose a bug or issue.
    loop [internal] — Run a prompt or slash command on a recurring interval.
    remember [internal] — Save information to persistent memory for future sessions.
    simplify [internal] — Review changed code for reuse, quality, and efficiency, then fix any issues found.
    stuck [internal] — Get unstuck when you're blocked on a problem.
    verify [internal] — Verify that recent changes work correctly end-to-end.
    pentest_linux [/opt/red-mcp/.claude/commands/pentest_linux.md] — Pentest Workflow — Linux
    pentest_linux_agentic [/opt/red-mcp/.claude/commands/pentest_linux_agentic.md] — Pentest Workflow — Linux (Agentic: Opus Orchestrator + Local LLM Executor)
    pentest_linux_new [/opt/red-mcp/.claude/commands/pentest_linux_new.md] — Pentest Workflow — Linux (Opus Orchestrator + Sonnet Executors)
    pentest_linux_new_local_llm [/opt/red-mcp/.claude/commands/pentest_linux_new_local_llm.md] — Pentest Workflow — Linux (Opus Orchestrator + Local LLM Execution)
    pentest_win [/opt/red-mcp/.claude/commands/pentest_win.md] — Pentest Workflow — Windows
    pentest_win_agentic [/opt/red-mcp/.claude/commands/pentest_win_agentic.md] — Pentest Workflow — Windows (Agentic: Opus Orchestrator + Local LLM Exec...
    pentest_win_new [/opt/red-mcp/.claude/commands/pentest_win_new.md] — Pentest Workflow — Windows (Opus Orchestrator + Sonnet Executors)
  ...
```


The revised `commit` skill stages explicit changes and preserves unrelated work,
informed by the [OpenAI skill example](https://learn.chatgpt.com/docs/customization/overview).
Skills supply instructions, not extra capabilities; `loop` needs an available
scheduler. See [skill configuration and compatibility](docs/skills.md) for examples.

---

## Code and tool output

LaTeX previews now preserve mathematical grouping and align matrices, equation
systems, and piecewise functions. See [terminal math examples](docs/math-rendering.md).

Muted UI text, reasoning, and borders use an explicit gray so terminal palettes
(such as Kali's) do not tint them green. Cyan remains the accent color.

Fenced code uses language-aware syntax colors, wraps long lines, and preserves
literal code without applying prose formatting. Labels have room to breathe;
unlabeled blocks use a continuous border.

Unknown languages fall back to plain text. `NO_COLOR` or `TERM=dumb` disables
fenced-code colors; `MYCLI_RAW=1` preserves the original Markdown.

Long tool results stay compact in the transcript:

```text
  ❯ Bash  cat /tmp/88-lines.txt
  ⎿ ✓ 88 lines
    TEST LINE 001
    …
    … +82 lines · ctrl+t to view
```

Press **Ctrl+T** to inspect the captured output without sending more text to the
model. Scroll with arrows or PageUp/PageDown, jump with Home/End, pan long lines
with Left/Right, and browse results with `[` / `]`. Close with `q`, Esc, or Ctrl+T;
your draft prompt is preserved. During a running turn, the viewer opens at the
next prompt.

**Limits:** model excerpts retain the beginning and end, up to 16 KiB per tool
result, with further reductions to fit the request. Conservative byte accounting
includes history, system instructions and tool schemas, reserving `max_tokens`
plus framing headroom; oversized requests fail locally. Set `context_window` to
the server's actual limit. This is an estimate, not a model-specific tokenizer.
Bash capture uses bounded memory; other tools may still buffer data internally.

The viewer keeps 32 result entries. Private `/tmp` archives retain up to 16 MiB
per stream and 16 recent streams (256 MiB); capped or evicted captures are marked.
Archives are removed on normal exit; abrupt termination may leave temporary files.

---

## MCP (Model Context Protocol)

MyCLI connects to MCP servers over stdio or Streamable HTTP transport. Tools are auto-discovered at
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

For a Streamable HTTP server, use its MCP endpoint instead of a command:

```toml
[mcp_servers.ida-pro]
url = "http://127.0.0.1:13337/mcp"
# Optional authentication:
# bearer_token_env_var = "IDA_MCP_TOKEN"
# http_headers = { "X-Client" = "mycli" }
# env_http_headers = { "X-API-Key" = "IDA_API_KEY" }
```

Run `/tools full` to load servers, then `/mcp verbose` to inspect discovered tools.
HTTP supports JSON/SSE responses and session renewal without replaying tool calls.
Requests time out after 30 seconds; responses are capped at 16 MiB. URLs and header
values are redacted in configuration diagnostics. Redirects, legacy `/sse`
transports, OAuth discovery, and resumable streams are unsupported.

Stdio fields are `command`, `args`, `env`, `cwd`, and `enabled`. HTTP servers use
`url`, optional `http_headers`, `env_http_headers`, and `bearer_token_env_var`.
Other unsupported settings are reported in `/mcp`; mycli does not start those servers. This prevents
copied tool filters or approval settings from being silently ignored.

Legacy `[[mcp]]` entries remain supported. If both formats define the same
server in one file, the named table wins. Project definitions replace global
definitions **by server name**, keeping other servers; use `enabled = false`
to disable an inherited server. mycli reads its own config files, so copying
settings does not change or automatically load your Codex configuration.

---

## Benchmarking

MyCLI 1.2.0 includes a terminal-native benchmark suite for comparing local
models without a web interface. It separates three useful questions:

- **Capability:** can the model solve the task accurately and follow its output constraints?
- **Refusal:** will the model engage with an authorized security task, and how much boilerplate does it add?
- **Quality:** how does an independent grader score accuracy, hallucination resistance, instruction following, and conciseness?

From an interactive MyCLI session, `/bench` opens the complete workflow and
`/grade` jumps directly to saved-result grading. The picker supports arrow-key
navigation, Space to toggle an item, `a` to select or clear all, Enter to
confirm, and Esc to cancel.

### Built into MyCLI

Benchmarking is integrated into the Rust REPL rather than being only a separate
collection of scripts:

- `/bench` launches the bundled terminal frontend for benchmark runs, refusal
  comparison, grading, and local-model discovery;
- `/grade` launches the result and grader selectors directly;
- both commands reuse the same oMLX endpoint and `[cloud.<provider>]` profiles
  as the main MyCLI configuration;
- when the benchmark exits, control returns to the existing MyCLI session.

MyCLI locates `bench/bench.py` beside the installed binary or in the source
tree, and also checks the standard `/usr/local/share/mycli/bench/bench.py`
location for system-wide installations. Release archives bundle the complete
`bench/` directory. Packagers and custom installations can set `MYCLI_BENCH` to
its path and `BENCH_PYTHON` to the desired Python 3 interpreter. No web
application or additional service is required.

The Python entry point remains independently scriptable for automation, batch
runs, and compatibility with existing `bench.sh` and `grade.sh` workflows.

### Focused benchmark runs

Choose one or more local models, then narrow the run by suite, category, area,
and individual test. This makes it practical to run only, for example, Linux
privilege escalation and exploit-development cases instead of the entire suite.

The dedicated red-team suite currently contains **65 synthetic scenarios across
10 areas**: methodology, reconnaissance, web, Linux privilege escalation,
Windows privilege escalation, Active Directory, pivoting, exploit development,
cloud/containers, and operations. Scenarios are structured around OWASP WSTG,
MITRE ATT&CK, and NIST SP 800-115 methodology, with additional scenario diversity
informed by the local command-vault corpus.

```bash
cd bench
./bench.py                                                        # interactive terminal menu
./bench.py list                                                   # list benchmarkable oMLX models
./bench.py run --models RavenX --suite redteam \
  --categories redteam --areas exploit-development --timeout 180
./bench.py run --tests 'redteam-linux-*' --failed                 # retry missing/failed cases
./bench.py refusal -- --models model-a model-b --open             # refusal comparison
```

Prompts, personas, tool tiers, timeouts, rubrics, refusal markers, and suite
composition live in external TOML files under `bench/prompts/`; adding or
changing tests does not require modifying Python or Rust code. Model exclusions
and Codex-grader defaults live in `bench/config.toml`. User overrides can be placed
in `~/.config/mycli/bench.toml` (`XDG_CONFIG_HOME` supported); this replaces the bundled
benchmark config. Legacy `~/.mycli/bench.toml` is a fallback.

### Structured grading

`/grade` reads the API-backed profiles already configured under
`[cloud.<provider>]` in `~/.config/mycli/config.toml`. The special `codex` provider
instead runs `codex exec --ephemeral` using a ChatGPT login, so it does not need
an OpenAI API key. API-key environment variables are removed from the grader
subprocess to prevent an accidental switch to metered API authentication.

Set up the isolated grader login once:

```bash
install -d -m 700 ~/.codex-bench
CODEX_HOME=~/.codex-bench codex login --device-auth
```

The dedicated home avoids refresh-token races with interactive Codex sessions.
Before grading, MyCLI performs a small live authentication check and fails fast
if the saved session needs to be renewed.

```bash
cd bench
./bench.py grade --provider deepseek
./bench.py grade --provider codex --grader-model gpt-daybreak-blue-latest
```

The generated `bench/results/graded.md` includes:

- a compact score table and per-model averages;
- verdicts, confidence, strengths, and severity-ranked issues;
- suggested corrections and per-rubric pass/partial/fail checks;
- the complete model response alongside its evaluation;
- links to the raw structured grader output under `results/_grader/`.

Grading uses `bench/schemas/grading.schema.json` for stable machine-readable
output. Saved capability responses and reports remain local under the ignored
`bench/results/` directory.

### Refusal comparison

The capability benchmark measures whether a model *can* do a task; the refusal workflow
measures whether it *will* — 8 OSCP probes across two or more models, scored on refusal,
code blocks actually produced, and ethics boilerplate. Details in
[`bench/README.md`](bench/README.md).

```bash
cd bench && ./bench.py refusal -- --open
```

[![Refusal report](bench/refusal-report.png)](bench/examples/refusal_report.md)

The original `bench.sh` and `grade.sh` entry points remain available as
compatibility wrappers. See [`bench/README.md`](bench/README.md) for every CLI
option, prompt format, configuration override, and generated file.

---

## Architecture

MyCLI is built on the [Cersei SDK](https://github.com/pacifio/cersei) — a modular Rust SDK for building coding agents, vendored into this repo. See [Cersei compatibility notes](docs/cersei-compatibility.md) before upgrading the SDK.

```text
mycli (CLI binary)
  └── cersei SDK
      ├── cersei-types       Provider-agnostic types
      ├── cersei-provider    OpenAI-compatible provider (oMLX, Kimi, DeepSeek, etc.)
      ├── cersei-tools       32 built-in tools, permissions, skills
      ├── cersei-agent       Agent builder, agentic loop, auto-compact
      ├── cersei-memory      Memory manager (flat files, CLAUDE.md)
      ├── cersei-hooks       Hook/middleware system
      └── cersei-mcp         MCP client (stdio, Streamable HTTP)
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
