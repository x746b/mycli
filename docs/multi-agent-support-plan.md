# Multi-agent support implementation plan

Status: proposed; implementation has not started.

Date: 2026-09-22.

## Goal

Make MyCLI an observable workspace for supervised work across cloud and local
models. The user should be able to watch an orchestrator and several workers,
open any worker's conversation, send instructions while it is working, and
understand whether those instructions were received.

The initial validation workloads are bounded coding, documentation research,
debugging, offline artifact analysis, and explicitly authorized pentesting or
CTF targets. Pentesting workers must inherit one target scope and shared safety
policy; spawning another agent must never broaden either.

Example: an orchestrator uses a configured cloud profile while researcher,
worker, and debugger sessions use configured DeepSeek profiles. Names and model
identifiers are configuration, not hardcoded assumptions about availability.

## Product requirements

- Every agent has a stable identity, task, model profile, conversation, and state.
- Every agent can be watched and addressed directly by the user.
- The orchestrator sees worker progress and completion without repeated polling.
- Messages show queued, delivered, or failed status; receipt is not inferred from
  an agent's prose.
- Tool calls and bounded output are visible alongside assistant responses.
- Interrupting a turn, stopping an agent, and closing its window are distinct.
- Closing or reconnecting a viewer does not silently discard a conversation.
- Parallelism has explicit limits and does not assume local inference capacity.
- Existing single-agent use remains available without tmux or a supervisor.
- Crew size follows the evidence: zero workers when recon is inconclusive, one
  for a dominant path, and a bounded portfolio for distinct promising paths.
- The user can inspect why each worker was selected, its evidence and budget,
  and every pause, stop, or reprioritization decision.

Observability means responses, actions, outputs, and explicit progress summaries.
It does not depend on exposing private model reasoning.

## Current implementation and constraints

Source review baseline: MyCLI `8003275`; upstream Cersei
`708c5055845ba6c682d92960cec99e3adbcca3e1`. These findings are from source
inspection, not end-to-end performance measurements. Recheck before implementation.

| Area | Current state | Implication |
| --- | --- | --- |
| `crates/mycli/src/repl.rs` | `build_tools` does not register agent/task tools | There is no enabled team runtime in the CLI |
| `crates/cersei-agent/src/agent_tool.rs` | Child execution is awaited to completion | Existing delegation is not managed background execution |
| `crates/cersei-agent/src/runner.rs` | Tool calls execute sequentially; streaming control receiver is unused | Control delivery and scheduling need explicit integration |
| `crates/cersei-tools/src/tasks.rs` | Process-global in-memory records; stop changes a status field | Task records do not own execution or cancellation |
| `crates/cersei-tools/src/send_message.rs` | Inbox registry exists without consumption in the agent loop | Successful enqueue does not establish delivery |
| `crates/cersei-tools/src/bash.rs` | Execution awaits completion/timeout; no interactive stdin or job handle | Managed jobs require a separate execution lifecycle |
| `crates/cersei-agent/src/lib.rs` | `run_stream` extends a borrowed reference through unsafe code | Correct ownership before introducing more background execution |
| Skills discovery | Claude command and skill formats are already recognized | Loading a workflow does not guarantee its runtime dependencies exist |

The existing child builder also hardcodes `AllowAll` and rebuilds tools from the
standard catalogue. It must not become the production delegation path without
correct policy inheritance and support for configured custom/MCP tools.

Upstream Cersei has an `Arc`-based streaming ownership fix and concurrent batch
execution, but its agent/task tools do not supply the complete lifecycle needed
here. Follow [Cersei compatibility guidance](cersei-compatibility.md): evaluate
small backports separately, preserving MyCLI's provider, cancellation, streaming,
reasoning, and compaction behavior. Do not make a whole-SDK upgrade a prerequisite.

### Documentation follow-up

The [Cersei documentation](https://cersei.pacifio.dev/docs) provides additional
components to evaluate before implementation:

- [Background Tasks](https://cersei.pacifio.dev/docs/background-tasks) describes
  in-memory task tracking without persistence across agent restarts. This supports
  the need for durable lifecycle management beyond task bookkeeping.
- [Workflows Overview](https://cersei.pacifio.dev/docs/workflows-overview) documents
  a separate `cersei-workflows` engine with serializable graphs, parallel branches,
  and streamed execution events. The page shows version `0.2.1`; this is a
  documentation reference, not a verified dependency recommendation.
- [PentestAgent](https://github.com/GH05TCREW/pentestagent) exposes crew mode,
  named child agents, and manual spawn/despawn controls. Treat it as a product
  and coordination reference; verify its source and interaction semantics before
  adopting an implementation pattern.

The workflow engine was not covered by the initial source review and is not in
MyCLI's inspected workspace. Evaluate it before building equivalent pipeline
features. Explicit pipelines could complement the supervised agent runtime;
their documented features do not establish support for direct conversations with
running workers, delivery receipts, durable recovery, or attachable terminal views.

During Phase 0, map each candidate capability to its actual crate, source revision,
and tests. Record whether to reuse it, backport a focused change, or implement the
missing behavior locally. Check API compatibility and lifecycle semantics rather
than treating documentation examples as proof of runtime behavior. Preserve the
existing compatibility policy and keep workflow-engine adoption optional.

## Architecture

Use a local supervisor to own execution. Terminal interfaces are clients of that
supervisor, including the orchestrator's view. Tmux arranges those clients; it is
not the task scheduler or source of truth.

```text
Team overview       Orchestrator pane       Worker panes
       \                    |                   /
          Local client protocol and event replay
                            |
                       Supervisor
          sessions / messages / budgets / jobs
                            |
                  Agent runtime adapters
                  /                    \
           Cersei runtime          Optional future backend
                  |
         Configured cloud/local providers
```

Start with a local Unix socket restricted to the owning user. Do not introduce a
network listener in the first release. Version the protocol and report client /
supervisor incompatibility clearly.

A proposed `mycli-runtime` crate should contain supervisor state and orchestration
independent of terminal rendering. Keep provider-specific request behavior in
the provider layer and runtime fixes in the relevant vendored Cersei modules.
Confirm the crate boundary after the ownership spike; avoid moving unrelated code.

### State and persistence

Keep these entities distinct:

- **Team:** workspace, profiles, membership, concurrency limits, and budgets.
- **Agent session:** stable ID, display name, parent, role, profile, conversation,
  tool policy, and workspace assignment.
- **Task:** assignment and outcome, including a reference to its owning session.
- **Turn:** one active processing cycle in a session; at most one per session.
- **Job:** a managed external process with output, exit state, and cancellation.
- **Message:** sender, recipient, origin, content, delivery state, and correlation ID.

Use SQLite for lifecycle metadata, message delivery, and a durable ordered event
log. Reuse existing transcript persistence where practical. Large output belongs
in bounded artifact files, referenced by events. Define one authoritative owner
for each record; avoid two independent writers updating the same transcript.

Events carry team/session/turn IDs as applicable, sequence numbers, timestamps,
and typed payloads. Subscribers resume from a cursor and deduplicate replay.
Persist accepted user input before acknowledging it. Runtime events, rather than
model-written status text, determine whether execution is running or stopped.

On supervisor restart, reconcile unfinished work. Do not label it successful or
automatically replay commands whose effects are unknown. Conversation recovery
does not imply a running process survived. The first release may mark such jobs
interrupted/unknown and require explicit continuation.

### Agent and tool configuration

Snapshot the resolved model profile and effective tool configuration when starting
an agent. Record provider, model, reasoning settings, context limits, and profile
identity without persisting credentials in transcripts or events.

Children inherit the parent's effective permissions unless the user explicitly
configures a narrower policy or authorizes a change. Preserve custom tools and MCP
bindings through a tool factory or shared registry; do not reconstruct them solely
by name from `cersei_tools::all()`.

Assign mutating coding workers separate worktrees where appropriate. Shared files
and stateful tools need explicit ownership. Do not blindly parallelize every tool
call in a model response. Initially prioritize concurrent independent sessions.

### Messaging and control semantics

| Action | Required behavior |
| --- | --- |
| Message | Persist and queue for the next safe point; show queued then delivered |
| Interrupt and redirect | Cancel the active turn, record partial/unknown outcomes, then begin the redirected turn |
| Follow up | Start a new turn in the same conversation when idle; queue if busy |
| Stop | Cancel agent execution and associated jobs according to ownership; report incomplete termination |
| Close pane | Detach the viewer; leave execution unchanged |
| Stop team | Request cancellation for the team and report remaining live work |

A safe point is before the next model request or after the current tool operation
has settled. Do not promise injection into an already-running provider generation.
Expose interrupt-and-redirect for immediate intervention.

Delivered means the message was durably incorporated into the recipient's input;
it does not mean understood or acted upon. Retry with stable message IDs to prevent
duplicate incorporation. Preserve origin so user instructions, worker reports,
and tool output do not become indistinguishable instructions.

User redirection of a worker should create a visible event for its orchestrator.
The user retains final control; contradictory queued assignments must be surfaced
rather than silently replacing the user's direction.

### Managed jobs

The supervisor owns process handles, process groups, output drains, and completion
events. A long-running job returns a handle rather than occupying the conversation
until exit. Output capture is bounded, with explicit truncation and artifact links.

Define job ownership, cancel escalation, time limits, stdin/PTY support, and exit
reporting. Start with noninteractive jobs; add interactive terminals only after
the cancellation and output lifecycle works. Do not claim reliable job survival
across supervisor restarts in the first milestone.

### Limits and responsiveness

Enforce team and provider concurrency limits, including a conservative configurable
limit for local inference. Track aggregate usage as well as per-agent usage.
Budgets must include child work; a child's own turn limit is insufficient.

Slow viewers must not block workers. Use bounded subscriber queues and replay from
durable events after a subscriber falls behind. Keep control requests responsive
while tokens or large tool outputs are streaming.

## Adaptive pentesting crew orchestration

For authorized pentesting and competitive lab workflows, use a recon-led adaptive
crew instead of a fixed swarm. More findings should not automatically mean more
workers. The planner selects a small portfolio of credible, sufficiently independent
tracks, while preserving the option to run a single fully observable worker.

```text
scope + safety policy
          |
   automated/manual recon
          |
 normalized findings + artifacts
          |
 candidate tracks and evidence links
          |
 score + interference analysis + resource limits
          |
 proposed crew plan (user-visible)
          |
 0..N observable workers, normally capped at 4
          |
 verified progress / stagnation / success / side effects
          |
 pause, stop, continue, or replan
```

### Recon contract

Recon scripts such as `win_recon.sh` should feed the planner through a versioned
result contract. Prefer JSON or JSONL emitted alongside the human-readable log.
When only text exists, an importer may normalize it, but it must retain links to
the original output and mark parser-derived claims as such.

A finding contains at least:

- target and scope identity;
- observation type, value, timestamp, and source command/tool;
- evidence artifact reference and parser confidence;
- whether collection changed remote state or may be incomplete;
- correlations such as host, service, account, domain, or application identity.

The orchestrator should not repeatedly rerun expensive or stateful recon because
another worker cannot see the result. Findings and artifacts are team-readable;
credentials remain in a scoped secret store and are referenced by opaque handles.

### Candidate tracks and weighted selection

The planner converts findings into explicit candidate tracks. Each track records:

- hypothesis and intended outcome;
- supporting and contradicting evidence;
- prerequisites and a concrete verification step;
- confidence, expected path value, exploitability, estimated time/cost, and
  overlap with other tracks;
- affected resources and interference class;
- initial worker role/profile, tool needs, budget, success proof, and stop rules.

Use normalized scores as decision support, not as false precision. An initial
configurable ranking can combine evidence confidence, path value, exploitability,
coverage of a distinct hypothesis, and fit within the available time, then subtract
cost, uncertainty, safety, and interference penalties. Persist the component scores
and a short rationale so the user can understand and override the result.

Crew selection is a constrained portfolio decision:

- choose no worker when no track reaches the configured evidence threshold;
- choose one worker when a track clearly dominates, a detected CVE has strong
  precondition matches, or shared-state constraints make concurrency unsafe;
- choose two to four workers when several credible tracks are meaningfully
  independent and the provider, hardware, scope, and safety budgets allow it;
- queue lower-value tracks rather than spawning them merely to fill capacity;
- avoid selecting redundant workers unless their assignments test genuinely
  different hypotheses or implementations.

The maximum is configurable and must not default upward merely because resources
are available. The plan shown before spawning includes selected and deferred tracks,
scores, evidence, conflicts, estimated budgets, and the reason for crew size.
The user may edit that plan or allow policy-based automatic execution.

### Worker contract and progress

Every worker starts with an immutable assignment envelope containing the authorized
scope, role, track hypothesis, evidence references, effective permissions, shared
constraints, resource budget, success proof, stop conditions, and progress cadence.
Later messages amend the assignment through visible events rather than silently
rewriting its original objective.

Workers report structured progress at safe points:

- current hypothesis and action;
- new evidence or contradiction, with artifact references;
- remaining blocker and next intended action;
- budget consumed and whether the track is advancing;
- side effects, acquired resource leases, and requested help.

Progress reports complement the live transcript and tool stream. They do not hide
the underlying work. The user can enter a worker pane, add information, redirect
the next step, interrupt the active turn, or pause/stop the track.

### Interference and shared-state controls

Parallel safety depends on affected state, not only on whether tracks have different
names. Classify intended actions before dispatch:

| Class | Examples | Default policy |
| --- | --- | --- |
| Read-only | service/version checks, LDAP queries, SMB listing, certificate enumeration | Parallel within rate limits |
| Shared authentication | password validation, Kerberos requests, spraying | Shared lockout/rate budget and coordinated scheduling |
| Stateful | session use, ticket/cache changes, file upload, service or ACL modification | Resource lease; serialize conflicting actions |
| Disruptive | reset/restart, destructive exploit attempt, broad credential attack | Explicit policy or user authorization |

Represent affected resources with stable keys such as `host:dc01`,
`account:alice`, `service:dc01/mssql`, and `domain:corp.local`. The supervisor owns
leases and aggregate rate/attempt budgets. Workers declare resources before a
state-changing action; the runtime queues or rejects conflicts and explains why.

AD-specific policy should coordinate account lockout thresholds, authentication
rates, Kerberos ticket/cache ownership, domain-controller affinity, shared sessions,
and modifications to directory objects or services. Read-only enumeration can
usually proceed concurrently, while actions with cross-track effects require a
lease or serialized checkpoint. Unknown side effects raise the interference class.

Tool permissions remain necessary but are insufficient: permission answers whether
an action may occur, while a lease answers whether it may occur concurrently now.

### Rabbit-hole control and replanning

Each track has an elapsed-time, turn, inference, command, and optional authentication
budget. Add track-specific limits such as maximum PoC adaptation attempts. A worker
requests an extension with evidence and a revised estimate; it does not silently
consume the team's remaining time.

Reconsider a track when it repeats the same failure, exhausts a prerequisite,
produces no material evidence for a configured interval, discovers unsafe shared
state, or is dominated by a newly verified path. The orchestrator recommends one
of `continue`, `narrow`, `pause`, `stop`, or `replace` and exposes the rationale.
User direction has priority and is delivered to both the worker and orchestrator.

Pause is a first-class state: settle or cancel the active operation, release leases
that cannot safely be retained, preserve the conversation and artifacts, and make
the track resumable. Stop additionally ends owned jobs and records any outcome that
remains unknown. Neither action is represented by changing a bookkeeping field alone.

### Success and stop-on-signal behavior

A worker success is a structured milestone containing the claim, verification
method, evidence/artifacts, confidence, current access or capability, relevant side
effects, and required next step. The supervisor distinguishes `candidate`,
`verified`, and `invalidated`; model prose alone cannot mark a track verified.

On verified success, the orchestrator evaluates other tracks by overlap:

- pause workers pursuing the same objective or modifying the same resources;
- stop clearly obsolete work after active operations settle;
- allow independent, useful evidence collection to continue within the team budget;
- retain workers whose results can validate or safely strengthen the viable path;
- show the proposed actions and allow the user to message, pause, stop, resume, or
  override any worker directly.

Pausing is preferable when the successful path may still fail during the next
stage. If it is later invalidated, the previous crew plan and preserved workers
support fast resumption rather than reconstructing their state.

### Pentesting orchestration events

Add typed events for `ReconCompleted`, `FindingRecorded`, `TrackProposed`,
`CrewPlanProposed`, `CrewPlanApplied`, `TrackProgress`, `LeaseRequested`,
`LeaseGranted`, `LeaseBlocked`, `MilestoneCandidate`, `MilestoneVerified`,
`TrackPaused`, `TrackResumed`, `TrackStopped`, and `CrewReplanned`. Each event
links to its evidence, actor, policy decision, and correlation IDs where applicable.
These events drive both recovery and the overview UI.

### Existing workflows as orchestration policy sources

Use mature single-threaded workflows as domain-policy baselines and add
parallelism as an orchestration overlay. Do not translate an entire Markdown
skill into Rust or duplicate its product-specific reasoning in the supervisor.

The inspected `pentest-linux-new` and `pentest-linux-new-paralel` pair demonstrates
this separation. Both retain the same high-level state machine:

```text
Prepare -> Acquire -> Triage -> Prove access -> Post-access -> Complete
                            +
              observable parallel-routing policy
                            |
              1-2 bounded independent workers
```

The parallel workflow adds reusable policy concepts without replacing the base
workflow: incremental `recon_events.jsonl` consumption, evidence-ranked live
leads, concrete next proofs and expected outcomes, attempt/time budgets, an
independence gate, central ownership of shared shells/listeners/tunnels, scoped
worker ledgers, structured checkpoints, operator steering, rabbit-hole limits,
and impact-lock preemption when a path proves useful access.

Treat these assets as battle-shaped policy candidates rather than automatically
proven runtime behavior. For the inspected Linux parallel workflow, repository
history shows a recent dedicated implementation and no discovered automated test
covering its routing decisions. Validate it through recorded solves and deterministic
replay before using it as the canonical reference implementation.

Keep four layers separate:

| Layer | Responsibility |
| --- | --- |
| Domain workflow | Define recon, validation, foothold, escalation, and completion |
| Orchestration policy | Decide whether, when, and how workflow branches run concurrently |
| Supervisor runtime | Own sessions, messages, leases, jobs, cancellation, and persistence |
| TUI/client | Expose evidence, activity, decisions, and direct operator control |

Add optional machine-readable orchestration metadata beside a skill rather than
requiring the supervisor to interpret every instruction in prose:

```text
SKILL.md                    Human/model workflow and evidence logic
orchestration.toml          Events, routing gates, budgets, conflicts, milestones
references/*.md             Domain-specific policies and operator guidance
```

The metadata references named workflow events and capabilities; it does not embed
commands, credentials, or a box solution. Skills without metadata continue to run
as ordinary single-agent workflows.

The first domain adapter should faithfully reproduce the existing Linux contract
before generalizing it:

1. Consume incremental recon events and preserve their source artifacts.
2. Produce normalized lead cards with evidence, next proof, expected outcome,
   budget, affected resources, and status.
3. Keep one observable worker when one lead dominates.
4. Spawn a second worker only when both leads are evidence-backed, independently
   provable, bounded, non-conflicting, and neither has already proven access.
5. Process checkpoints and operator messages while both tracks run.
6. Trigger impact lock on verified read, execution, authentication, shell, or flag
   access; pause the competing branch and preserve its state.
7. Reapply the same routing gate after foothold when independent privilege paths
   appear.

After parity is established, extend the adapter from its conservative two-lead
contract to the configurable 0-4 weighted portfolio. Windows and AD adapters add
their own evidence types, resource keys, and interference rules while sharing the
same supervisor lifecycle.

### Command Vault as the solved-case evidence source

Wire Command Vault into the planner as a provenance-preserving corpus of successful
and failed episodes from solved labs. It supplies empirical priors for track ranking,
budgets, common prerequisites, and rabbit-hole detection. Current-target evidence
still has priority over historical similarity.

Keep retrieval modes explicit:

| Mode | Retrieval boundary |
| --- | --- |
| Replay/evaluation | Exact solved cases, complete routes, failures, timing, and orchestration decisions |
| Active solve | Cases matched only by observed products, versions, errors, configurations, binaries, groups, protocols, or techniques |

During an active solve, prohibit queries by the current box name and exclude known
solutions or writeup prose for that target. This preserves the existing workflow
boundary: historical evidence helps choose how to investigate an observed signal;
it does not reveal the current lab's route. Log the query, filters, returned case
IDs, and provenance so retrieval remains auditable.

Normalize each solved run into a case episode containing:

- initial structured findings and evidence references;
- candidate tracks plus their component scores and selection/defer decisions;
- proof attempts, outcomes, false prerequisites, and failed approaches;
- elapsed time, attempts, tool/inference cost, and stagnation points;
- pivots, preemption, user steering, worker pause/stop, and interference events;
- verified foothold/root milestones and the minimal supporting proof;
- whether parallelism improved time-to-proof or added redundant work.

Redact flags, credentials, tokens, and unnecessary target identifiers. Store opaque
secret references and outcome classes rather than replayable sensitive values.
Version the episode schema and preserve links to the original authorized run ledger
where retention policy permits.

Return compact prior cards to the planner instead of injecting entire writeups:

```text
Technique: exposed repository
Support: 18 solved episodes
Prerequisites: downloadable objects or directory listing
Useful next proofs: recover config; inspect focused commit history
Median path to credential: 3 actions
Common dead end: exhaustive source review before secret search
Interference: read-only / low
Provenance: case IDs and evidence references
```

Prior cards contribute configurable components such as historical success rate,
typical time-to-proof, prerequisite failure rate, and known interference. They may
change a track's rank or initial budget, but they cannot promote a contradicted
prerequisite or mark current-target impact as verified.

At completion, write the new episode back to Command Vault after review/redaction.
Record which tracks were proposed, which were selected, why they succeeded or
failed, when workers were redirected or preempted, and whether the chosen crew size
helped. This closes the feedback loop without allowing worker prose to become
unreviewed canonical truth.

## Interface: tmux first

Provide a team overview and one attachable agent view per session. The overview
shows role, profile, current task, execution state, elapsed time, and unread events.
Each agent pane shows its transcript, current tool activity, expandable output,
and a composer clearly labeled with the recipient.

A pentesting team view should make selection rationale, live activity, shared-state
conflicts, and direct control visible without opening every transcript:

```text
┌ TEAM: lab-01  scope: 10.10.11.0/24  elapsed 00:18:42  budget 61% ┐
│ ORCHESTRATOR · cloud-profile     PLAN v3 · 3 active · 1 queued  │
│ Verified: web foothold candidate; proposing pause of Track T2   │
├──────────────────────────────┬───────────────────────────────────┤
│ T1 CVE validation · worker-a │ T2 ADCS analysis · researcher-b  │
│ RUN tool: adapt-poc          │ PAUSE PENDING · safe point       │
│ score .86 · attempt 2/3      │ score .72 · lease domain:corp    │
│ msg: [type here]             │ msg: [type here]                 │
├──────────────────────────────┼───────────────────────────────────┤
│ T3 web path · debugger-c     │ QUEUE / CONFLICTS / MESSAGES     │
│ VERIFIED candidate · proof ↗ │ T4 spray deferred: lockout risk  │
│ [verify] [pause peers]       │ 2 unread · 1 lease blocked       │
└──────────────────────────────┴───────────────────────────────────┘
Keys: Enter inspect · m message · i interrupt · p pause · s stop · r resume
```

State labels must come from runtime events. Highlight stale progress, unknown
outcomes, pending messages, blocked leases, and incomplete cancellation. Selecting
a score opens its components and evidence; selecting a worker opens the complete
conversation and tool stream. Destructive or shared-state decisions identify the
requesting worker and affected resources.

The initial tmux adapter creates only MyCLI-owned sessions and panes. It must not
kill unrelated panes or assume pane numbers remain stable. Use stable agent IDs
for routing; never use `send-keys` as the message transport. Provide an ordinary
terminal attach mode when tmux is absent.

Candidate command surface, to be finalized during the protocol spike:

```text
mycli team start
mycli team status
mycli team attach <team-id>
mycli agent attach <agent-id>
mycli agent message <agent-id>
mycli agent interrupt <agent-id>
mycli agent stop <agent-id>
```

Prefer interactive input or an input file for message bodies. Keep credentials
out of command arguments and pane titles. Keep tmux optional so a native split-pane
UI or another window manager can later consume the same protocol. Amux is not yet
evaluated and is not a dependency of this plan.

## Delivery phases and acceptance gates

### Phase 0: ownership and lifecycle foundation

- Recheck source findings and existing regression coverage.
- Evaluate the documented task and workflow components against versioned source
  and tests; record reuse/backport/build decisions before duplicating features.
- Fix streaming ownership and establish explicit task handles/cancellation.
- Specify protocol events, state transitions, and persistence ownership.
- Use deterministic fake providers for lifecycle tests without inference costs.

Gate: dropping a viewer cannot invalidate a running agent; cancellation works
during a silent provider request; single-agent behavior remains intact; the
component evaluation records supported capabilities and unresolved gaps.

### Phase 1: observable manual team — first vertical slice

- Implement a local supervisor and durable session/event records.
- Launch one orchestrator and two user-selected workers manually.
- Add overview, per-agent attach, message delivery receipts, follow-up, and
  interrupt-and-redirect.
- Add the optional tmux layout adapter.

Gate: on a bounded coding task the user can watch all three sessions, message
either worker, interrupt one, and reattach a closed pane without losing history.
An orchestrator receives a worker progress update. A worker cannot weaken the
parent's effective permission policy.

### Phase 2: dependable jobs and recovery

- Add managed background commands, incremental output, and verified termination.
- Add restart reconciliation, artifact retention, and replay under backpressure.
- Exercise provider failure, rate limiting, tool timeout, and viewer disconnection.

Gate: no false stopped/completed states; unknown outcomes remain explicit; accepted
messages survive restart; stalled clients do not stall execution.

### Phase 3: supervised delegation

- Expose bounded worker creation and task assignment to the orchestrator.
- Preserve provider profiles, tool bindings, workspaces, and shared budget limits.
- Deliver progress/completion events and support continued worker conversations.
- Keep recursive delegation disabled initially; allow the user to inspect and
  stop any worker independently.

Gate: a bounded repository task runs with observable parallel workers, shared
limits hold, and direct user redirection remains visible to the orchestrator.

### Phase 4: adaptive authorized pentesting crews

- Define the recon finding and candidate-track schemas plus import adapters for
  representative Linux, Windows, and AD recon output.
- Implement the first policy adapter from `pentest-linux-new` plus its observable
  parallel-routing overlay; preserve single-worker behavior when its fan-out gate
  does not pass.
- Define optional versioned `orchestration.toml` metadata without making it a
  prerequisite for ordinary skills.
- Implement explainable track ranking and interference-aware portfolio selection.
- Add assignment envelopes, progress/milestone events, budgets, leases, pause and
  stop-on-verified-signal behavior.
- Implement the pentesting overview and user-editable proposed crew plan.
- Add Command Vault prior-card retrieval with strict active-solve filters, query
  provenance, episode redaction, and reviewed post-solve writeback.
- Keep automatic state-changing actions behind the effective scope/safety policy.

Gate: fixture-driven recon yields a deterministic explained plan: one strong CVE
signal produces one observable worker, while independent findings produce a bounded
crew. Conflicting AD actions cannot run concurrently. A verified path pauses
overlapping workers, preserves resumable state, and remains overridable by the user.
The Linux adapter reproduces its existing two-lead routing contract before wider
fan-out, and active-solve retrieval cannot return the current target's known route.

### Phase 5: workflow compatibility and performance

- Report missing tools and unsupported workflow metadata before execution.
- Test workflow portability separately from Markdown discovery.
- Benchmark sequential work against two and four workers where hardware permits.
- Evaluate a native split-pane interface using the same supervisor protocol.

Gate: publish completion quality, wall-clock time, usage, failure rate, and control
latency. Keep parallelism optional where it increases cost or slows completion.

## Validation strategy

Use meaningful state-machine and integration tests for cancellation races, duplicate
messages, restart recovery, policy inheritance, custom/MCP tool preservation,
output bounds, workspace isolation, and provider concurrency limits. Preserve the
existing provider, streaming, compaction, and session regressions.

For adaptive pentesting, use captured or synthetic recon fixtures and mock tools to
test scoring, dominance, portfolio diversity, lease conflicts, lockout budgets,
stagnation, milestone verification, and stop-on-signal races without touching a
live target. Add authorized lab smoke tests only after deterministic cases pass.

Build a replay corpus from completed workflow ledgers and recon event streams. For
each case, compare the sequential baseline, the existing conservative routing
policy, and the adaptive planner on spawn timing, lead independence, time-to-first
verified proof, wasted attempts, preemption latency, and operator steering delivery.

Evaluate Command Vault with leave-one-case-out replay: exclude the evaluated case
and all aliases from retrieval, reveal only its recon events over time, and measure
whether prior cards improve ranking and budgeting without exposing its solution.
Test exact-target, alias, and writeup-leakage filters as security boundaries.

Run a small real-provider smoke test only with configured access and bounded cost.
Do not use startup time or token throughput as a substitute for task correctness.
Set performance targets after collecting a baseline on the user's actual hardware
and provider profiles; no model-performance claims are established by this plan.

## Deferred decisions

- Native split-pane UI versus continued tmux use, based on the vertical slice.
- Interactive PTYs and persistent execution across supervisor restart.
- More than one level of delegation and cross-machine supervisors.
- An optional Codex execution adapter. Evaluate official SDK/App Server interfaces
  separately; do not assume a ChatGPT subscription or model alias maps to a direct
  provider API. Preserve backend-specific controls and capability reporting.

Completion of this plan means delivering an inspectable, steerable team runtime
with honest lifecycle reporting. It does not mean matching every feature or the
task-solving quality of another agent product.
