# Persona modernization proposal

Status: proposal, not applied to the shipped catalog.

The proposed replacement is [system-prompts.proposed.toml](system-prompts.proposed.toml).
It targets the user's mixed Qwen 3.6/3.8, GLM 5.3 Flash, and DeepSeek 4.1 Flash setup
without assuming that an oMLX or ds4 model alias implies particular template behavior.
The local serving configurations were not available here, and this proposal has not
been evaluated against those endpoints.

## Content changes

| Section | Proposed change and reason |
| --- | --- |
| code | Replace the permission-to-chat preamble with implementation, debugging, testing, and honest reporting goals. Direct answers remain covered once in tier guidance. |
| redteam | Replace assumed authorization and unconditional compliance with evidence, explicit scope, impact, and remediation. A persona cannot establish assessment scope. This is a substantive behavior change, not merely shortening. |
| blueteam | Focus on evidence versus hypotheses, provenance, confidence, and validation instead of a list of security product categories. |
| data | Replace blanket one-liner preference with reproducibility, schema/unit checks, preservation, and consistency checks. |
| math | Request a checkable derivation and verified result instead of exhaustive narration or “never skip steps.” |
| agentic | Remove evaluation roleplay and blind literalism; retain format constraints, correct tool use, recovery from failures, and honest completion claims. |
| neutral | Empty prompt, always suppresses tier style. Still receives tool guidance and runtime context; it is not a completely empty system message. |
| tiers | Remove duplicated tool inventories. Retain the simple tier's full-file update distinction, and add focused verification and preservation of unrelated changes. |
| style | Scope brevity to the final response's result, evidence, and remaining issues. Avoid instructions about internal reasoning length or simulated expert panels. |

These are design hypotheses, not measured quality improvements. Each proposed persona
and tier is shorter in characters than its original counterpart. This is not a
claim about tokens across different tokenizers.

## Keep behavior controls separate

Use provider/template configuration for thinking, sampling, and tool-call serialization.
Do not put temperature settings, forced `<think>` blocks, or model-specific control tags
in generic personas. Qwen documents template-level thinking controls and tool-call
format requirements; DeepSeek documents its own thinking-mode request and message
handling. This supports keeping persona content independent of transport, but does
not verify the exact models or proxies used here.

References: [Qwen quickstart](https://qwen.readthedocs.io/en/latest/getting_started/quickstart.html),
[Qwen function calling](https://qwen.readthedocs.io/en/latest/framework/function_call.html),
[DeepSeek thinking mode](https://api-docs.deepseek.com/guides/thinking_mode/).

The existing reasoning plan's measured word counts are observations from its particular
runs. They do not establish that prompt ordering alone caused the difference. Compare
several runs before attributing behavior changes to a single instruction.

## Evaluate before adopting

Compare the shipped catalog, this proposal, and Neutral on the same loaded model,
tool tier, sampling parameters, token budget, and fresh conversation. Include:

- A focused Rust bug fix with executable checks.
- A code review with an intentionally missing requirement.
- A data transformation with missing values and known expected output.
- A mathematical problem with an independently verifiable answer.
- An open design question, such as reconnect versus buffering in a telemetry client.
- A strict JSON response and a failed-tool recovery case.

Record correctness, tool-call validity, verification quality, unwanted tool calls,
latency, and input/output token usage. Reasoning length alone is not a quality score.
Repeat across the actual oMLX and ds4 model IDs; do not generalize from one Qwen variant.
Use `MYCLI_PROMPTS=/absolute/path/to/docs/system-prompts.proposed.toml` to opt in for
a separate session. The shipped file and embedded fallback remain unchanged.
