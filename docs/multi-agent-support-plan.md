# Multi-agent support implementation plan

Status: proposed; implementation has not started.

Date: 2026-09-22.

## Goal

Make MyCLI an observable workspace for supervised work across cloud and local
models. The user should be able to watch an orchestrator and several workers,
open any worker's conversation, send instructions while it is working, and
understand whether those instructions were received.

The initial validation workloads are bounded coding, documentation research,
debugging, and offline artifact analysis. This plan defines general runtime and
interface capabilities, not autonomous offensive workflows.

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

## Interface: tmux first

Provide a team overview and one attachable agent view per session. The overview
shows role, profile, current task, execution state, elapsed time, and unread events.
Each agent pane shows its transcript, current tool activity, expandable output,
and a composer clearly labeled with the recipient.

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
- Fix streaming ownership and establish explicit task handles/cancellation.
- Specify protocol events, state transitions, and persistence ownership.
- Use deterministic fake providers for lifecycle tests without inference costs.

Gate: dropping a viewer cannot invalidate a running agent; cancellation works
during a silent provider request; single-agent behavior remains intact.

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

### Phase 4: workflow compatibility and performance

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
