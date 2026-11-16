# MyCLI benchmark improvements: implementation plan

Status: benchmark, live trial, and Bash auto-background increments implemented through MyCLI 2.3.0 on 2026-11-16. Remaining phases below are planned, not implemented.

## Implementation progress (2026-11-16)

### Completed

- [x] CLI `--tools suite|simple|medium|full|auto|inherit` and equivalent menu selection.
- [x] Backward-compatible `suite` default and `advanced` normalization to `full`.
- [x] Explicit `--max-turns` forwarding and positive CLI budget validation.
- [x] Requested tool policy and explicit tier recorded; fingerprints include tool policy, timeout, and turn limit.
- [x] Unique directories for new runs under `results/runs/<run-id>/`, with a basic manifest and latest-run pointer.
- [x] Default grading/menu discovery uses the latest run; explicit `--results` supports older/legacy directories.
- [x] stdout/stderr streamed to retained log files, with JSON execution status and return code.
- [x] Nonzero exits no longer receive a successful completion tick; completed processes explicitly remain unverified.
- [x] Timeout/cancellation process-group cleanup and preservation of partial output.
- [x] `--failed` refuses to overwrite an existing result with a different task/settings fingerprint.
- [x] README documents new options, storage, and current limitations.
- [x] Four focused unit tests pass: policy/command construction, fingerprints, parser, and subprocess evidence/status including timeout.
- [x] Two live smoke tests through `/bench` in tmux session `mycli`, using the already loaded `Ornith-1.5-122B-oQ5e-mtp` model with 30-second limits.

Live evidence:

| Test | Settings | Observed result | Duration | Run ID |
|---|---|---|---|---|
| File write | `simple`, 3 turns | `smoke.txt` contained exactly `OK`; reply `DONE`; exit 0 | 6 seconds | `20261116T104553-d2bba6be` |
| Brief reply | `inherit`, 2 turns | reply exactly `OK`; startup diagnostics reported `medium`; exit 0 | 2 seconds | `20261116T104616-47a9266e` |

The smoke artifact and reply were manually checked. These are not evidence of an automated outcome-verifier implementation. The original interactive session returned to its prompt.

### Partial implementation and remaining work

- [ ] **Phase A partial:** runtime effective-config/tool export, named profiles, and explicit active-session inheritance remain unimplemented. `auto`/`inherit` resolved tiers remain unknown in JSON; diagnostics can show the tier, but the runner does not parse them.
- [ ] **Phase B partial:** storage currently retains the existing per-model layout inside each run, rather than the proposed opaque trial IDs. Basic manifests do not capture full binary/configuration/skill identities. Safe opaque model keys are still needed; currently unsafe model directory names are rejected.
- [ ] Retry attempts are not immutable: `--failed` reuses compatible latest-run records. `--resume`, `--rerun-failed`, and run-aware immutable grading records remain planned.
- [ ] Atomic record writes, complete interrupted-run lifecycle metadata, aggregate infrastructure-failure exit semantics, and `--fail-on-task-failure` remain planned.
- [ ] **Phase C pending:** task kinds, fixtures, environment adapters, private executable verifiers, structured events, enforced usage budgets, and agentic coding pilot.
- [~] **Phase D content started:** universal live Linux and Windows agent briefings are in `bench/scenarios/htb_live/`. IP is required and box name is optional. Linux/Windows adapters, milestone checks, controlled skill discovery, and a runnable live HTB track remain pending.
- [ ] **Phase E pending:** repetitions, randomized order, paired comparisons, profiles, and fuller reporting.

Implementation files: `bench/bench.py`, `bench/README.md`, and `bench/tests/test_runner.py`. No Rust runtime changes or cyber task content were made in this increment.

The phase specifications and acceptance criteria below remain the target design. They must not be read as claims that those features already exist. The cyber-content handover is in [bench_cyber_handover.md](bench_cyber_handover.md).

### Operator steering during live trials (2026-11-16)

- [x] Esc during a live single-shot turn interrupts that turn and opens an operator guidance prompt. Submitting guidance starts another model turn in the same session; an empty Enter asks the model to continue from current evidence.
- [x] Foreground tool execution is cancelled on interruption; already tracked background tasks keep running. The live PTY transcript records the guidance and model response.
- [x] Ctrl+C remains the deliberate trial stop. A short local tmux smoke test interrupted a Bash `sleep`, accepted the operator's `test` guidance, and showed the same model session respond afterward. No HTB target was contacted.

### Long Bash commands in live trials (2026-11-16)

- [x] MyCLI 2.3.0 enables auto-background Bash by default in medium/full tiers; live benchmark children inherit `MYCLI_BASH_AUTO_BACKGROUND=0` when the operator disables it. In medium/full tiers, a Bash command finishing within 60 seconds returns normally; a longer one hands off the same process to the existing Background manager and returns its task ID.
- [x] A 300-second health check reports running state and time since output without treating silence as a hang or stopping the job. The model/operator can inspect with `BackgroundOutput` or `/tasks output`.
- [x] Explicit shorter timeouts and cancellation before handoff retain process-group cleanup. Simple tier stays on the original Bash tool.
- [x] Astra coding task ran through `codex exec --cyber-access-program standard`; focused Rust and Python tests passed. The collaboration-agent API could not disable Daybreak for Astra directly.

### Tmpfs live workspaces (2026-11-16)

- [x] The live runner verifies `/tmp` is `tmpfs` and creates a unique mode-0700 `/tmp/bench-<UTC timestamp>` workspace per trial.
- [x] The MyCLI process starts with that cwd and `-C`, and TMPDIR/TMP/TEMP point there.
- [x] A private copy of the selected MyCLI configuration and prompt/skill catalogs is placed under the trial workspace; XDG session and memory files stay on tmpfs. The source configuration remains untouched.
- [x] Both agent-facing task briefs forbid creating a separate `/tmp/htb/<box>` workspace and direct recon helper output into the supplied trial directory.
- [ ] Persistent benchmark result metadata and the private full transcript remain under `bench/results/runs/` for operator review; this is intentional and distinct from the scratch workspace.

### Live session visibility and MyCLI 2.2.0 (2026-11-16)

- [x] Live `/bench` trials now use a PTY and stream the entire session into the current terminal/tmux pane while recording a combined `*.session.log`.
- [x] Live MyCLI renders reasoning, complete tool arguments and results, and final output. Tool permission review is the default; auto-approval is explicit (`--yes` or a menu choice). Ctrl+C stops the trial and invokes process-group cleanup.
- [x] Session logs and live result files use private file permissions. The operator can review the saved transcript after the pane scrollback is gone.
- [x] MyCLI package and banner report `2.2.0`; release binary rebuilt and installed at `/opt/mycli/mycli`.
- [x] A short local PTY smoke test in a separate tmux session showed the `2.2.0` banner, live thinking, a permission dialog accepting Enter, full Bash input/output, and final response. No HTB box was contacted.
- [ ] Automated flag verification, active-machine API checks, and checkpoint/steering controls remain pending. Streaming shows progress but is not an outcome verifier.

### Live HTB menu integration (2026-11-16)

- [x] `/bench` menu includes "Run live HTB task" for Linux or Windows, one local model, required current IP, optional box name, and tool policy. It uses one self-contained briefing per platform. The menu applies 3600-second/100-turn caps without asking for them; CLI overrides remain available.
- [x] `bench.py live --os ... --ip ... [--name ...] --model ...` provides the same path for scripts.
- [x] Current IP is validated against supported HTB machine ranges and must route through `tun0`; no target traffic occurs during this local route check.
- [x] Current live briefings are self-contained. The runner does not inject Codex or Claude Code HTB workflow skills; it permits relevant installed helper scripts. Earlier explicit skill injection was removed because it depended on a different agent harness.
- [x] Live runs use the normal run directory, logs, and manual-verification status. Seven Python tests pass, including prompt rendering and route checks.
- [ ] Active-machine identity, fresh reset, private verifier, controlled skill discovery, and automatic milestone scores remain pending. The menu does not make a completed process a verified solve.

### Live HTB benchmark content handoff (2026-11-16)

The operator spawns each box and supplies its current HTB VPN IP. The self-contained model-facing briefs are [Linux](../bench/scenarios/htb_live/linux.md) and [Windows](../bench/scenarios/htb_live/windows.md). The optional box name is context only. They describe scope, a compact evidence-driven test cycle, mandatory background recon, relevant helper catalogs and post-access enumeration, and the user/root goals without disclosing a solve route. `render.py` fills in the current IP. No operator-only scenario notes are stored in the live scenario directory. The live runner disables MyCLI’s Skill tool while keeping other configured tools available.

The earlier handover's synthetic-first recommendation remains applicable for stable cross-model scores. The live-box track is an additional operator-directed test with fresh resets and evaluator-held proofs.

## 1. Objective and architectural decision

Extend the existing benchmark frontend to evaluate both response quality and interactive task completion. Preserve historical capability/refusal workflows. Add explicit tool selection, reproducible run records, executable outcome verification, and Linux/Windows lab task packs.

Use one runner and reporting pipeline. TOML suites define ordinary benchmark tasks; the live HTB path uses self-contained task briefings and installed helper scripts. Do not build a second benchmarking engine inside an HTB skill. Detailed scenario content can be authored separately after the task schema and verifier contract stabilize.

Distinguish three questions in reports:

1. How well does the model answer a supplied question?
2. How reliably does the model complete a task with a fixed tool configuration?
3. How well does the user's configured MyCLI installation perform, including its skills and integrations?

Do not combine these into one headline score.

## 2. Verified starting point

Source inspection of the current checkout establishes:

- `bench/bench.py` provides suite loading, filters, local model selection, execution, interactive menus, cloud grading, and Markdown reports.
- `load_suite()` merges `[defaults]` into each test and supports included TOML files. Existing categories, areas, rubrics, and test IDs can be reused.
- `run_benchmark()` explicitly passes `-t <test tier>`, defaulting to `simple`, plus persona, model, a fresh working directory, and `-y`.
- `bench/prompts/redteam.toml` defaults to `simple` and contains synthetic written scenarios. Much existing content asks for explanation or illustrative commands rather than verified action.
- `crates/mycli/src/repl.rs::build_tools()` gives `simple` Read, Write, and Bash. It already permits execution and iterative testing. `medium` adds structured editing/search and background jobs; `full` adds WebFetch, Skills, and configured MCP integrations.
- `config.rs::resolve_tool_tier()` resolves `auto` to `medium` for local providers and `full` for cloud providers.
- `repl.rs::launch_benchmark()` launches Python without passing the live REPL configuration. Saved configuration inheritance and active-session inheritance are different requirements.
- Single-shot mode already expands `/skill <name> <arguments>`. Skill expansion is distinct from exposing the model-callable Skill tool.
- Current results occupy `results/<model>/<test>.md` and `.raw`; another configuration of the same test overwrites them.
- Current fingerprints cover selected task fields, not the resolved runtime configuration.
- A nonempty response currently receives a success tick even if the process exits unsuccessfully. stderr is captured but not saved. File counts are not correctness checks.
- `--failed` currently means missing/stale/runner-failed results, not necessarily tasks that failed a grader or outcome verifier.
- The interactive grader discovers the legacy result layout directly; storage changes must update execution, grading, and menus together.

A working directory is not an isolation boundary. Because even `simple` exposes Bash, withholding a structured tool does not prevent equivalent shell or network activity.

## 3. Scope and non-goals

First release: tool control, durable run records, correct execution status, diagnostics, and backward-compatible grading. Second release: fixtures, verifiers, traces, and a small coding pilot. Third release: bounded HTB Linux/Windows tasks and workflow-skill comparisons.

Do not rewrite the refusal runner, add distributed workers, require a database, or replace the existing cloud judges. Keep Python standard-library compatibility where practical. Keep model execution sequential initially; concurrent models sharing a lab target would invalidate comparisons.

Cloud models as evaluated subjects are a later extension. Current capability model discovery is local; cloud grading support does not imply cloud subject support.

## 4. Tool selection and precedence

Add `bench.py run --tools {suite,simple,medium,full,auto,inherit}`.

| Value | Behavior |
|---|---|
| `suite` | Preserve existing per-test tier, including defaults; fallback `simple` |
| `simple`, `medium`, `full` | Override every selected test with that explicit tier |
| `auto` | Pass MyCLI's automatic tier policy explicitly |
| `inherit` | Omit the tier argument and use the child process's loaded configuration |

Default to `suite` for compatibility. Normalize legacy `advanced` to `full`; reject unknown tiers before launching any model. An explicit CLI override wins over a named benchmark profile; the profile wins over suite settings. When `inherit` or `auto` is selected, do not silently fall back to a task's tier.

Add the same choices to `interactive_run()` and show the requested policy in the start summary. Record both requested and resolved values per trial. Do not claim `auto` is a controlled tool comparison: its result depends on provider.

### Saved settings versus live REPL state

Initially define `inherit` precisely as saved child-process configuration. Document that changing `/tools` in the current session is not automatically forwarded.

Then extend the Rust launcher to send a versioned, allowlisted settings snapshot to Python using a private temporary file or equivalent structured channel. Include the active tool policy and selected nonsecret generation settings, not credentials or the entire Config object. An explicit benchmark CLI option must override the forwarded snapshot. Preserve benchmark model selection; do not replace every selected model with the REPL model.

Expose active-session inheritance as a separately labeled menu choice or option. Never silently change the meaning of `inherit` depending on launch location. Include snapshot cleanup on normal exit and failures.

### Named profiles

After basic overrides work, add `[profiles.<name>]` entries in `bench/config.toml` for repeatable combinations of tools, turn limits, timeouts, reasoning settings, skill policy, and environment adapter. Proposed examples: `coding-controlled`, `htb-controlled`, and `configured-installation`.

A profile is a settings bundle, not a security boundary. Any MCP allowlist or network policy must be enforced by MyCLI or the environment adapter; a label in TOML is insufficient. Validate unsupported settings instead of silently ignoring them.

## 5. Run storage and configuration capture

Introduce a versioned layout, using filesystem-safe opaque keys for model/trial paths and retaining display names in JSON:

```text
bench/results/runs/<run-id>/
  manifest.json
  summary.md
  trials/<trial-id>/
    result.json
    stdout.log
    stderr.log
    events.jsonl
    response.md
    artifacts.json
    verifier.json
  grading/<provider-key>/<grader-key>/...
```

Generate collision-resistant run IDs. Keep mutable workspaces outside immutable result records; copy or index retained artifacts after execution. Do not expose other trials' results, solutions, or grader files to the evaluated environment.

Manifest fields:

- Schema version, run ID, timestamps, invocation, selected test IDs, repetition count, and ordering seed.
- MyCLI source revision and binary identity; record an unavailable revision explicitly. A source revision alone does not prove the binary was built from it.
- Suite/task content hashes, profile hash, fixture version/hash, verifier version/hash, and environment adapter/version.
- Requested model/provider identity and actual resolved identity when available.
- Requested and effective tool tier, actual registered tool names, MCP connection status, and missing required capabilities.
- Persona/system-prompt identity and hashes, skill inventory/content hashes, invocation mode, and reasoning/generation settings.
- Timeout, turn/token/cost budgets where enforceable, and network policy.

Use an allowlist for configuration export. Never serialize API keys, authorization headers, complete environment variables, or credential-bearing MCP configuration. Preserve operational traces with appropriate local access permissions; redaction must not destroy the evidence needed for verification.

Add a machine-readable MyCLI capability/config export rather than scraping human `--show-config` output. Startup metadata should describe actual resolved tools, including MCP connection failures. Optional integrations may be reported as degraded; missing task-required integrations make the trial unavailable, not a model failure.

Fingerprint the normalized task plus effective settings, fixture, skills, verifier, and binary identity. Separate a task-definition hash from an execution-configuration hash so reports can explain what changed. Do not include credentials in either.

## 6. Trial lifecycle and status semantics

Implement an explicit lifecycle:

`validate -> provision/reset -> health check -> execute -> verify -> collect -> teardown -> finalize`

Always run teardown after provisioning has begun, including timeouts and interrupts. Preserve a partial result if any later stage fails. Write result JSON atomically.

Represent execution and outcome separately:

- Execution: `completed`, `timeout`, `process_error`, `cancelled`, `infrastructure_error`.
- Outcome: `pass`, `partial`, `fail`, `not_evaluated`.
- Include process return code, termination reason, elapsed time, and diagnostic references.

Nonempty stdout is evidence, not success. Preserve partial output on timeout, handling both byte and text subprocess output. Keep stderr even when stdout is empty. A verifier may observe a completed milestone after timeout, but the trial remains a timeout and cannot count as successful completion within budget.

Use process-group lifecycle management on supported POSIX platforms: start a new session, terminate the group on timeout/cancellation, wait a bounded grace period, then kill remaining group members. Environment teardown must also clean up remote services or detached jobs that process-group termination cannot cover. Document platform-specific behavior.

Define exit codes: zero means the requested run completed operationally; infrastructure/configuration failures produce nonzero. Add `--fail-on-task-failure` for CI that should fail on unmet task outcomes. Persist per-trial details regardless of aggregate exit policy.

Replace ambiguous retry behavior with `--resume <run-id>` for missing/interrupted compatible trials and `--rerun-failed <run-id>` for new trials linked to unsuccessful prior trials. Retain `--failed` as a documented compatibility alias with a warning when its legacy meaning differs. Never mutate historical trial outcomes.

## 7. Agentic task schema

Continue accepting existing TOML tests unchanged. Add an explicit task kind, defaulting to `response`; agentic tasks use `workspace` or `lab`.

Illustrative proposed schema, not currently supported syntax:

```toml
[defaults]
category = "coding"
tier = "medium"
kind = "workspace"

[[test]]
id = "agentic-parser-repair"
area = "debugging"
persona = "code"
prompt = "Reproduce the parser defect, fix it, and run the available tests."
rubric = "Assess whether the explanation matches the observed changes."
required_tools = ["Bash"]

[test.budget]
timeout_seconds = 300
max_turns = 30

[test.environment]
adapter = "local-container"
fixture = "parser-repair-v1"
network = "disabled"

[test.verifier]
kind = "command"
argv = ["python3", "verify.py", "--json"]
timeout_seconds = 30
location = "evaluator"

[test.skills]
mode = "disabled"
```

Resolve fixture and verifier identifiers through trusted benchmark metadata. Never interpolate model output into host shell commands. Execute argv arrays; use explicit cwd and environment. Reject duplicate IDs, invalid kinds, unsafe paths, missing fixtures, contradictory requirements, and unsupported budgets before running.

Verifiers live outside the agent-accessible environment. For code tasks, transfer candidate artifacts into a clean verification environment with private tests. Merely placing hidden tests in a sibling directory on the same unrestricted host does not keep them hidden.

Define verifier JSON with schema version, overall outcome, named milestones, evidence references, and verifier errors. A crashing verifier is an infrastructure error, not proof that the model failed. Keep verifier execution bounded and independent of text grading.

## 8. Tracing and budgets

Add an opt-in structured event sink in MyCLI, independent of terminal rendering. Before adding new plumbing, inspect existing session/event infrastructure and reuse stable records where possible.

Events should include trial/run correlation IDs, timestamps, model requests, tool start/completion, tool names and argument references, error status, usage counters when provided, and final termination. Do not require private chain-of-thought. Avoid making benchmark storage dependent on ANSI-rendered output.

Retain raw evidence with bounded output handling. Large tool results should reference artifacts, with explicit truncation metadata. Stream subprocess output to files to avoid accumulating unbounded output in Python memory.

Enforce wall-clock and turn limits first; MyCLI already exposes `--max-turns`. Token/cost limits require actual runtime support and accounting, not post-hoc labels. Report unsupported usage metrics as unavailable rather than zero. Record provider token usage separately from locally estimated counts; price-based cost estimates must identify their rate table and date.

## 9. Grading and reports

Keep cloud grading for response quality and methodology. Executable verifiers determine objective task outcomes. A judge must not overrule a failed deterministic check because the prose is convincing.

Update `grade_results()`, interactive grading discovery, summary generation, and compatibility wrappers for run-aware storage. Support `grade --run <run-id>` and retain explicit legacy result-path reading. Store grader model, rubric hash, raw structured output, and grading timestamp per run. Regrading creates a new grading record.

Primary agentic report: successful trials within budget divided by eligible trials, alongside numerator and denominator. Show infrastructure errors separately and also show total requested trials so exclusions cannot hide poor environment reliability.

Report time and usage distributions, milestone completion, timeout counts, and repeated-run success rate. Label repeated-trial success separately from pass-at-k; do not claim the latter without its defined sampling protocol. Use equal task weighting for a suite summary and provide per-task results; do not let repeated easy tasks dominate.

Keep knowledge, coding, Linux, Windows, live-lab, and configured-installation tracks separate. Compare models under matching configuration hashes or explicitly show the differences. Do not rank efficiency without also showing success rate.

## 10. HTB task packs and skills

Use the same runner and verifier contract for `htb-linux.toml` and `htb-windows.toml`. Detailed scenarios are a later content task; start with synthetic resettable lab fixtures and a small bounded pilot.

Suggested progression:

1. Inspect supplied enumeration artifacts and select an evidence-backed next step.
2. Repair a broken lab helper and prove it against a fixture.
3. Recover from a reproducible failed approach using newly observed evidence.
4. Linux privilege escalation from a fixed initial foothold.
5. Windows/AD investigation from a fixed initial identity and permissions.
6. Multi-stage full-machine solve after reset and scoring are reliable.

For each fixture specify initial state, authorized target set, health checks, reset ownership, credentials delivery, milestones, success evidence, and teardown. Avoid named-machine walkthrough answers as the core success criterion. Verify privileges or state through the evaluator, rather than trusting a model's claim or a copied flag alone.

Provide an environment adapter interface: prepare, reset, healthcheck, connection metadata, collect evidence, teardown. Implement one adapter first; do not assume Linux containers can reproduce Windows/AD behavior. Windows tasks may require VM snapshots or an externally managed reset service.

Skills are controlled inputs: disabled, explicitly invoked, or discoverable. Distinguish a prompt expanded via `/skill` from autonomous use of the Skill tool. Pin content and dependencies. Because current skill discovery includes configuration and working-directory paths, reproducible disabled/pinned modes may require a new MyCLI discovery-policy control; omitting `/skill` alone is insufficient.

Keep grading rubrics, expected secrets, and verifier code outside operational skill content. Compare with-skill and without-skill configurations on the same reset fixture and budget. Initially do not introduce multi-agent execution; if later tested, record it as a separate configuration with aggregate budgets.

Separate controlled research tasks from live-web tasks. State whether documentation, vulnerability references, and target walkthroughs are available. Enforce restrictions at the environment layer if they are part of the benchmark claim. WebFetch alone is not a guarantee of web-search capability; record the actual search integration when required.

Live HTB runs form a separate realism track. Record target version/state and reset results, prevent simultaneous trials against the same target, and exclude infrastructure outages explicitly. Do not make live HTB API access a prerequisite for local benchmark development.

## 11. Implementation work packages

### Phase A: tool control and configuration resolution

Files: `bench/bench.py`, `bench/config.toml`, `bench/README.md`; Rust `main.rs`, `config.rs`, and `repl.rs` for machine-readable resolution and later session forwarding.

- Extract pure tool-policy/command-building helpers.
- Add CLI and menu selection; preserve `suite` default and legacy normalization.
- Add requested/effective configuration metadata.
- Make saved versus active-session inheritance explicit.

Acceptance: existing suite commands retain their tiers; explicit override reaches every selected test; inherit omits `-t`; auto resolves correctly for the selected provider; invalid values fail before execution; no secrets appear in metadata.

### Phase B: trustworthy records and execution

Files: benchmark runner, new small storage/execution modules as needed, grading/menu readers, wrappers.

- Add versioned manifests and immutable trial directories.
- Fix status handling, preserve diagnostics and partial output.
- Add process cleanup, resume compatibility checks, and legacy readers.
- Update all grading/report entry points in the same change sequence.

Acceptance: two tiers of the same model/task coexist; a nonzero process with stdout is not marked completed; timeouts retain output and terminate child processes; interrupted runs remain inspectable; legacy results still grade.

### Phase C: executable coding pilot

Files: task schema validation, environment/verifier modules, fixture directories, `prompts/agentic-code.toml`, verifier schema, event-output integration.

- Implement one resettable workspace adapter and private verifier execution.
- Add a small pilot of approximately 5–10 tasks covering bug repair, regression handling, multi-file changes, and failed-test recovery.
- Add bounded traces and objective reports.

Acceptance: a plausible answer without a working artifact fails; deliberately broken artifacts fail; a correct reference solution passes; repeated trials start from identical fixtures; agent changes cannot modify private verifier logic.

### Phase D: HTB packs and skill comparisons

Files: Linux/Windows suite files, lab adapters, fixture metadata, skill discovery controls, documentation.

- Author bounded scenarios using the stabilized schema.
- Add milestone verifiers and explicit reset/health checks.
- Add paired skill configurations and a separately labeled live-lab track.

Acceptance: reference solutions pass from fresh resets; failed resets block execution; missing required MCP tools produce infrastructure status; skills and target state are identifiable in every record; one trial cannot inherit another's foothold or artifacts.

### Phase E: repeated comparisons and polish

- Add repetition count, reproducible randomized ordering, run comparison, and compatible reruns.
- Add profile selection to menus and exports.
- Document practical model-selection workflows and known sources of variance.

Acceptance: reports expose sample sizes, configuration differences, and exclusions; repeated runs never overwrite prior evidence; wrappers and `/bench` remain usable.

## 12. Validation strategy

Use pure unit tests for precedence, normalization, fingerprints, schema validation, and legacy/new record loading. Use a fake MyCLI executable for subprocess tests: successful output, empty output, nonzero exit with output, stderr-only failure, timeout with partial output, and a spawned child requiring cleanup.

Use fixture integration tests for reset determinism, reference-solution success, faulty-solution failure, verifier crash classification, artifact collection, and teardown on cancellation. Test grader readers without live API calls; reserve a small manual smoke run for real MyCLI/MCP behavior.

For Rust changes, add focused tests around argument parsing, safe metadata serialization, and session handoff precedence. Run the repository's relevant Python and Rust checks discovered during implementation; do not require every model or live lab to validate a storage change.

Before release, exercise legacy `bench.sh`, direct CLI, interactive menu, `/bench`, legacy grading, new run grading, and an interrupted/resumed run. Inspect generated metadata for credential leakage using synthetic secret markers rather than real secrets.

## 13. Migration and implementation boundaries

Do not rewrite or delete existing results. Read them through a legacy adapter and label absent metadata as unknown. New writes use the versioned layout. Preserve existing suite semantics unless an explicit agentic kind is selected. Update README examples only when the corresponding CLI exists.

Keep `bench.py` as the public entry point. Extract storage, execution, schema, and verifier helpers into small modules as each becomes necessary; avoid a broad framework rewrite before Phase A/B acceptance tests pass.

Decisions to settle during implementation: precise event API reuse, effective-config export format, available isolation backend, and Windows reset infrastructure. None prevents starting Phase A/B. Infrastructure-dependent tasks should remain unavailable with an explicit reason until their adapter is implemented.

## 14. Completion criteria

The improvement is complete when a user can select a controlled or inherited tool configuration, run repeatable interactive tasks, inspect actual outcomes and traces, compare models without overwritten records, and run Linux/Windows packs through the same framework. Existing response/refusal benchmarks and grading remain accessible. A successful run means verified completion within the declared budget, not merely a nonempty answer.
