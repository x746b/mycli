# Daybreak Sol handover: MyCLI cyber benchmark content

Historical handover: the current live Linux/Windows tasks use universal self-contained prompts in `bench/scenarios/htb_live/`, without injected HTB workflow skills or operator notes. Follow the current task files and `bench/README.md` for live runs; the skill-comparison requests below are superseded for those tasks.

Copy the prompt below into the next model's session. This document is a handover, not an instruction to launch lab activity now.

---

You are taking over cyber benchmark content design for `/opt/mycli`. Read `docs/bench_improvements.md`, especially its implementation progress section, and `bench/README.md` before editing. Inspect repository instructions and the current diff; preserve existing work. The user wants realistic interactive Linux/Windows HTB-focused evaluation alongside the existing response-quality suite. Detailed cyber content was deliberately deferred to you.

## Objective

Create reviewable Linux and Windows benchmark task packs, rubrics, fixture specifications, and verifier requirements that fit the existing suite architecture. Preserve the written reasoning baseline. Separate immediately runnable response tasks from future executable lab tasks. Do not create a second benchmark engine inside a skill.

## What exists

- Python entry point: `bench/bench.py`; suites in `bench/prompts/`.
- `benchmark.toml` includes `redteam.toml`, which already contains synthetic offensive-security scenarios with categories, areas, difficulty, tags, prompts, and rubrics. Inspect coverage before adding duplicates.
- `load_suite()` supports `[defaults]`, `[[test]]`, and includes.
- CLI/menu tool policies: `suite`, `simple`, `medium`, `full`, `auto`, `inherit`; turn and timeout overrides.
- Simple has Read, Write, and Bash: it can execute code and is not isolated. Medium adds structured editing/search and background jobs. Full adds WebFetch, Skills, and configured MCP tools. Actual available tools can also depend on provider/configuration; live medium diagnostics included WebSearch.
- `inherit` means saved child-process settings, not unsaved live REPL selections. Auto resolves local to medium and cloud to full.
- New runs use `bench/results/runs/<run-id>/`, with model subdirectories, a basic manifest, Markdown/raw responses, JSON execution metadata, and stdout/stderr logs.
- Process completion is explicitly unverified. Current grading remains qualitative cloud judging. Logs and file counts do not prove task success.
- Single-shot MyCLI supports `/skill <name> <arguments>` expansion. Skill discovery/pinning/disable controls for controlled evaluation are not implemented.

## Validation already performed

Four Python tests pass via `python3 -m unittest discover -s bench/tests -v`.

Two live tests ran through `/bench` in tmux session `mycli` using loaded model `Ornith-1.5-122B-oQ5e-mtp`:

- Simple tool write, 3 turns, 30-second timeout: exact `OK` artifact and `DONE` response; 6 seconds; run `20261116T104553-d2bba6be`.
- Inherit, 2 turns, 30-second timeout: exact `OK` response; saved configuration resolved to medium in diagnostics; 2 seconds; run `20261116T104616-47a9266e`.

The session was left idle. Do not assume its current model/state remains unchanged; inspect before interacting. The user requested only short tests with the loaded model. Do not run lengthy evaluations or change/load models for content development without further direction.

## Critical implementation boundary

Task kinds, environment adapters, fixtures, executable/private verifiers, milestone grading, structured tool traces, immutable retries, repetitions, and full configuration capture are PLANNED, not implemented. The current runner does not enforce the proposed agentic schema.

Do not add executable lab definitions to the auto-discovered `bench/prompts/*.toml` directory and assume unsupported fields will protect them from execution. Keep future lab specifications in a separate staging directory such as `bench/scenarios/`, clearly marked non-runnable. Promote them only after schema validation and adapter/verifier support exist. Do not include them from `benchmark.toml` yet.

Current Bash access on the host means sibling files are not hidden from the agent. A working directory does not isolate a verifier or protect answers. Specify an enforceable separation boundary for future executable tasks.

## Requested deliverables

1. Audit existing red-team coverage and write a concise gap map for Linux and Windows/AD. Distinguish response reasoning, practical tool use, recovery, and full workflow gaps.
2. Create a small, diverse first batch of staged Linux and Windows scenarios. Aim for roughly 4–6 per platform initially, with bounded objectives and explicit dependencies rather than a large superficial catalog.
3. For each scenario specify:
   - Stable ID, task kind, category/area, difficulty, and measured capability.
   - Exact agent-facing prompt and initial observations.
   - Initial foothold/identity, target boundary, and required environment state.
   - Fixture/version requirements, reset steps, health checks, and teardown ownership.
   - Required capabilities, tool policy, skill mode, and documentation/network availability.
   - Suggested wall-clock and turn budgets, labeled uncalibrated until measured.
   - Objective success criteria and named milestones, including evidence required for each.
   - Private verifier approach and reference-solution validation procedure.
   - Common failure modes, misleading success claims, and infrastructure-error conditions.
   - Dependencies blocking execution in the current runner.
4. Add immediately runnable response-only tests only where they add clear coverage, using the supported TOML schema and synthetic supplied evidence. Label these as reasoning tasks; do not imply they measure actual compromise or privilege escalation.
5. Define a small skill comparison matrix: no operational skill, explicitly invoked skill, discoverable skill. Explain controls needed to make each real. Skills must contain operational guidance, not benchmark answers or private grading logic.
6. Update the implementation plan with content completed, remaining infrastructure dependencies, and a short suggested next coding increment.

## Content priorities

Prioritize evidence-based next steps, interpretation of command output, debugging a lab helper, recovery after a failed hypothesis, Linux privilege reasoning from a supplied foothold, and Windows/AD identity/permission reasoning. Include plausible distractors and alternate valid approaches; grade observable outcomes rather than a single preferred command sequence.

Start with synthetic resettable fixtures. Full-machine and live HTB scenarios belong in a separate later track. Avoid copying named-box walkthroughs or rewarding memorized flags. A claimed shell, privilege level, or flag is insufficient without evaluator-observed evidence.

For web-enabled research tasks, distinguish documentation and vulnerability references from target walkthroughs. Identify how any access restriction will be enforced; prompt wording alone is not an enforcement mechanism.

## Validation and scope

Validate TOML parsing, unique IDs, include behavior, and supported fields for runnable tasks. Review staged specifications for complete reset and verifier contracts. Do not execute lab exploitation merely to author the handover content. Any eventual live test needs an identified authorized target and the applicable workflow instructions.

Keep implementation changes scoped to content and documentation unless the user asks you to take on runner work. Do not quietly implement a parallel runner or treat planned flags as available. Report what is runnable now, what is staged, how it was validated, and the next infrastructure dependency.
