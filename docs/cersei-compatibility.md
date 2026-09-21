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
