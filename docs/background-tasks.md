# Background commands — MyCLI 2.1.0

On `/tools medium` or `/tools full`, the model can start a shell command and
immediately continue other work. For example:

> Run the test suite in the background while you inspect the documentation.
> Check the test result before reporting that the change is ready.

These commands run locally with the same working-directory and environment
snapshot used by the session's Bash tool. They do not launch another model or
agent. Launches and stops use MyCLI's existing command-execution permission flow;
list and output calls require read-only permission.

## Tools and manual controls

| Model tool | Purpose |
| --- | --- |
| `BackgroundStart` | Start `command` with a short `description`; returns an ID immediately. Optional `timeout` is in milliseconds. |
| `BackgroundList` | List jobs belonging to the current session, with actual runtime status. |
| `BackgroundOutput` | Read current stdout/stderr, status, exit code, and archive paths using `id`. Does not wait. |
| `BackgroundStop` | Terminate the job's process group and wait for shell exit and output cleanup using `id`. |

The user can inspect or stop jobs independently of the model:

```text
/tasks
/tasks list
/tasks output <id>
/tasks stop <id>
/tasks stop all
```

MyCLI shows completion notices at the next REPL prompt. Completion does not start
a new model turn or interrupt an ongoing response. The model reads results with
`BackgroundOutput`; it should continue useful work between checks and verify the
final status before claiming success. `/tasks` controls and model tools are
available only in medium/full. The simple tier retains its original three tools.

## Lifecycle

- States are `running`, `stopping`, `completed`, `failed`, `stopped`, or `timed_out`.
  Only the runtime changes them. A nonzero or signaled exit is a failure.
- Jobs survive ordinary model/provider/persona changes within the same session.
  Foreground turn interruption leaves background jobs running.
- Switching to simple tools or resuming a different session stops that session's
  active jobs. A failed configuration switch retains the current configuration.
- Normal exit, EOF, errors returning from the REPL, and single-shot completion
  stop remaining jobs. The existing immediate Ctrl+C exit paths and SIGTERM
  signal process groups before exiting. An uncatchable kill or machine crash
  cannot guarantee cleanup.
- Stops use SIGKILL for the owned Unix process group, then wait for the shell and
  drain its pipes. A termination, wait, or capture error is reported as `failed`
  with an explanation. Stopping an already finished job preserves its result.
- Descendants remaining in the process group are also killed when the shell
  exits. Explicitly daemonized processes that leave the group are not managed.
- Input is closed: there is no interactive stdin or PTY. A background job's `cd`
  and environment changes do not modify the foreground shell state.

## Limits and output retention

- Maximum four simultaneous commands across this CLI process.
- Default timeout: one hour; explicit range: 1 ms to 24 hours.
- Maximum 32 records across sessions. The oldest finished records are evicted;
  running jobs are never evicted.
- Current stdout/stderr each retain a bounded 16 KiB head/tail excerpt. Private
  files under `/tmp/mycli-task-*` retain the first 1 MiB of each stream. Results
  include total byte counts and an explicit archive-truncation flag.
- Archives remain available while their record is retained, including after
  completion, and are deleted on eviction or CLI exit. Other Bash commands do
  not evict active background archives.
- Records and processes are not restored from saved conversations. Historical
  tool calls remain in the session transcript, but their IDs and archive paths
  may no longer exist after restart. Preserve useful results in project files
  before exiting if they must be retained.

This release implements process-local command management. The durable supervisor,
recovery, and agent delegation described in the multi-agent plan remain future
work.
