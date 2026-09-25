# Vendored Cersei compatibility

MyCLI uses its vendored Cersei crates. The maintainer reports that a previous
whole-SDK update introduced compatibility issues; the exact failure is currently
unknown. Treat upgrading the SDK as a separate change with its own compatibility
review, rather than a prerequisite for routine MyCLI features.

When changing these modules, check [upstream Cersei](https://github.com/pacifio/cersei)
for an existing fix. Prefer a small, reviewed backport when appropriate. Preserve
MyCLI's provider, streaming, cancellation, tool-call, and reasoning regressions.

## Local reasoning review — 2026-09-21

Inspected upstream revision `708c5055845ba6c682d92960cec99e3adbcca3e1`:

- [OpenAI provider](https://github.com/pacifio/cersei/blob/708c5055845ba6c682d92960cec99e3adbcca3e1/crates/cersei-provider/src/openai.rs):
  `reasoning_effort_for` gates forwarding to GPT-5 and o-series model IDs. It does
  not provide MyCLI's configurable local-profile reasoning catalog.
- [Agent effort module](https://github.com/pacifio/cersei/blob/708c5055845ba6c682d92960cec99e3adbcca3e1/crates/cersei-agent/src/effort.rs):
  generic effort/budget presets are separate from local template-specific values.

The local change therefore adds a model-bound optional level catalog to the existing
provider builder and shares validation with the CLI. It does not upgrade the SDK
or replace MyCLI's request encoding and streaming implementation.

## Background command review — 2026-09-25

Rechecked upstream HEAD `708c5055845ba6c682d92960cec99e3adbcca3e1` against the vendored
`cersei-tools/src/tasks.rs` and `bash.rs`, and the
[background-task documentation](https://cersei.pacifio.dev/docs/background-tasks).
Upstream remains at the source revision inspected for the multi-agent plan.

| Capability | Finding | Decision for 2.1.0 |
| --- | --- | --- |
| Task tools | Process-global in-memory records; create does not execute its prompt; stop changes a status field. Tools do not enforce session isolation. | Keep unregistered; do not present bookkeeping as running work. |
| Bash | Foreground command execution; no managed job handle or incremental status API. MyCLI already has local output and cancellation changes. | Preserve foreground behavior. |
| Tool integration | Existing `Tool`, `ToolContext`, permission levels, session shell snapshots, and tool-result metadata support a local adapter. | Reuse these interfaces in `mycli::background`. |
| Streaming ownership | Upstream's Arc ownership change is relevant to future background agents. Command workers here own process/data handles and never borrow `Agent`. | No streaming/runtime backport needed for this milestone. |
| Cron/workflows | Neither is needed for one-shot managed shell commands. | Defer scheduling and workflow-engine adoption. |

No SDK crates, provider encodings, dependencies, or streaming APIs are changed.
Validation covers command lifecycle, session isolation, limits, process-group
cleanup, foreground concurrency, and tier registration, plus workspace regressions.

## Explicit OpenAI project routing — 2026-09-25

The working Codex API launcher sets `OpenAI-Organization` and `OpenAI-Project`.
With the same key, MyCLI's tool/reasoning requests to GPT-6 Luna returned 403
without these headers; bounded direct requests succeeded when both were supplied.
Simple text-only success did not establish access for MyCLI's request shape.

Upstream's inspected OpenAI builder does not expose these routing fields. A small
local addition accepts optional `organization` and `project` in each cloud profile
and sets the corresponding headers on that provider's HTTP client. Responses and
Chat Completions share the client; selecting another profile constructs a fresh
client. No global environment override or credential substitution is introduced.
Regression tests cover transmission, client isolation, profile selection, and
invalid header rejection.
