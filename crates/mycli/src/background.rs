//! Session-owned shell jobs. No agent references or provider changes are needed.
//! Records and private output files live only for this CLI process.
use async_trait::async_trait;
use cersei_tools::{output, session_shell_state, PermissionLevel, Tool, ToolCategory, ToolContext, ToolResult};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::VecDeque, path::PathBuf, process::Stdio, sync::{Arc, LazyLock}, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

const MAX_RUNNING: usize = 4;
const MAX_RECORDS: usize = 32;
const ARCHIVE_BYTES: usize = 1024 * 1024; // per stream; at most 64 MiB retained
const MAX_TIMEOUT: u64 = 86_400_000;
const BASH_HANDOFF: Duration = Duration::from_secs(60);
const HEALTH_AFTER: Duration = Duration::from_secs(300);
pub static MANAGER: LazyLock<Manager> = LazyLock::new(Manager::default);

#[derive(Default)]
pub struct Manager {
    jobs: Mutex<VecDeque<Arc<Job>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum State { Running, Stopping, Completed, Failed, Stopped, TimedOut }

#[derive(Default)]
struct Output {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: u64,
}
impl Output {
    fn push(&mut self, bytes: &[u8]) {
        self.total += bytes.len() as u64;
        let head = bytes.len().min((output::MODEL_OUTPUT_BYTES / 2).saturating_sub(self.head.len()));
        self.head.extend_from_slice(&bytes[..head]);
        self.tail.extend(&bytes[head..]);
        let excess = self.tail.len().saturating_sub(output::MODEL_OUTPUT_BYTES / 2);
        self.tail.drain(..excess);
    }
    fn text(&self) -> String {
        let mut bytes = self.head.clone();
        if self.total > (self.head.len() + self.tail.len()) as u64 {
            bytes.extend_from_slice(b"\n[... output truncated; see bounded archive ...]\n");
        }
        bytes.extend(self.tail.iter());
        output::excerpt(&String::from_utf8_lossy(&bytes), output::MODEL_OUTPUT_BYTES)
    }
}

struct Progress {
    state: State,
    exit_code: Option<i32>,
    error: Option<String>,
    stdout: Output,
    stderr: Output,
    notified: bool,
    health_notified: bool,
    last_output_at: Option<Instant>,
    finished_at: Option<Instant>,
}
struct Job {
    id: String,
    session: String,
    description: String,
    cwd: PathBuf,
    timeout: u64,
    created_at: String,
    started_at: Instant,
    // TempPath owns deletion. Unlike the shared Bash archive cache, active
    // background archives cannot be evicted by unrelated foreground commands.
    paths: [tempfile::TempPath; 2],
    group: Arc<ProcessGroup>,
    progress: Mutex<Progress>,
    cancel: CancellationToken,
    finished: CancellationToken,
}
impl Job {
    fn snapshot(&self, include_output: bool) -> Value {
        self.snapshot_at(include_output, Instant::now())
    }
    fn snapshot_at(&self, include_output: bool, now: Instant) -> Value {
        let p = self.progress.lock();
        let now = p.finished_at.unwrap_or(now);
        let elapsed = now.saturating_duration_since(self.started_at);
        let mut value = json!({
            "id": self.id, "description": self.description, "status": p.state,
            "cwd": self.cwd, "created_at": self.created_at, "timeout": self.timeout,
            "exit_code": p.exit_code, "error": p.error,
            "stdout_bytes": p.stdout.total, "stderr_bytes": p.stderr.total,
            "output_files": [self.paths[0].to_path_buf(), self.paths[1].to_path_buf()],
            "archive_truncated": p.stdout.total > ARCHIVE_BYTES as u64 || p.stderr.total > ARCHIVE_BYTES as u64,
            "health": {
                "running": p.state == State::Running,
                "long_running": p.state == State::Running && elapsed >= HEALTH_AFTER,
                "elapsed_seconds": elapsed.as_secs(),
                "seconds_since_output": now.saturating_duration_since(p.last_output_at.unwrap_or(self.started_at)).as_secs(),
                "output_observed": p.last_output_at.is_some(),
            },
        });
        if include_output {
            value["stdout"] = json!(p.stdout.text());
            value["stderr"] = json!(p.stderr.text());
        }
        value
    }
}

fn health_notice(job: &Job, progress: &mut Progress, now: Instant) -> Option<String> {
    if progress.state != State::Running || progress.health_notified
        || now.saturating_duration_since(job.started_at) < HEALTH_AFTER
    {
        return None;
    }
    progress.health_notified = true;
    let elapsed = now.saturating_duration_since(job.started_at).as_secs();
    let idle = now.saturating_duration_since(progress.last_output_at.unwrap_or(job.started_at)).as_secs();
    Some(format!(
        "Background task {} is still running after {elapsed}s ({idle}s since output). Inspect with /tasks output {} or BackgroundOutput; silence alone does not prove a hang.",
        job.id, job.id
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    command: String,
    description: String,
    timeout: Option<u64>,
}

impl Manager {
    fn start(&self, input: Start, ctx: &ToolContext) -> Result<Arc<Job>, String> {
        if input.command.trim().is_empty() || input.command.len() > 65_536 {
            return Err("command must contain 1–65536 bytes".into());
        }
        if input.description.trim().is_empty() || input.description.len() > 1024 {
            return Err("description must contain 1–1024 bytes".into());
        }
        let timeout = input.timeout.unwrap_or(3_600_000);
        if !(1..=MAX_TIMEOUT).contains(&timeout) {
            return Err(format!("timeout must be 1–{MAX_TIMEOUT} milliseconds"));
        }
        let mut jobs = self.jobs.lock();
        if jobs.iter().filter(|job| !job.finished.is_cancelled()).count() >= MAX_RUNNING {
            return Err(format!("At most {MAX_RUNNING} background commands may run at once. Stop or wait for one first."));
        }
        let shell = session_shell_state(&ctx.session_id);
        let (cwd, env) = {
            let shell = shell.lock();
            (shell.cwd.clone().unwrap_or_else(|| ctx.working_dir.clone()), shell.env_vars.clone())
        };
        let archive = || tempfile::Builder::new().prefix("mycli-task-").tempfile_in("/tmp")
            .map(tempfile::NamedTempFile::into_parts).map_err(|e| e.to_string());
        let (stdout_file, stdout_path) = archive()?;
        let (stderr_file, stderr_path) = archive()?;
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", &input.command]).current_dir(&cwd).envs(env)
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(not(unix))]
        return Err("Background commands require Unix process groups".into());
        let mut child = command.spawn().map_err(|e| format!("Failed to start command: {e}"))?;
        let group = Arc::new(ProcessGroup(Mutex::new(child.id())));
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let job = Arc::new(Job {
            id: uuid::Uuid::new_v4().to_string(), session: ctx.session_id.clone(),
            description: input.description, cwd, timeout, created_at: chrono::Utc::now().to_rfc3339(),
            started_at: Instant::now(),
            paths: [stdout_path, stderr_path],
            group: group.clone(),
            progress: Mutex::new(Progress { state: State::Running, exit_code: None, error: None,
                stdout: Output::default(), stderr: Output::default(), notified: false,
                health_notified: false,
                last_output_at: None, finished_at: None }),
            cancel: CancellationToken::new(), finished: CancellationToken::new(),
        });
        if jobs.len() >= MAX_RECORDS {
            if let Some(index) = jobs.iter().position(|job| job.finished.is_cancelled()) { jobs.remove(index); }
        }
        jobs.push_back(job.clone());
        // No await between spawn and registration: cancellation of the tool call
        // cannot strand an unregistered process. Workers own data, never Agent.
        let running = job.clone();
        tokio::spawn(async move {
            let _completion = Completion(running.clone());
            let out = AbortOnDropHandle::new(tokio::spawn(drain(stdout, stdout_file, running.clone(), false)));
            let err = AbortOnDropHandle::new(tokio::spawn(drain(stderr, stderr_file, running.clone(), true)));
            let (mut state, mut result) = tokio::select! {
                biased;
                _ = running.cancel.cancelled() => (State::Stopped, None),
                status = child.wait() => (State::Completed, Some(status)),
                _ = tokio::time::sleep_until(running.started_at + Duration::from_millis(timeout)) => (State::TimedOut, None),
            };
            // Also stop descendants after a shell exits so pipes cannot remain open.
            let mut failure = group.kill().err().map(|e| format!("Process group termination failed: {e}"));
            if result.is_none() {
                result = match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                    Ok(result) => Some(result),
                    Err(_) => { failure = Some("Process exit could not be confirmed".into()); None }
                };
            }
            drop(group);
            let captures = tokio::time::timeout(Duration::from_secs(2), async {
                let (out, err) = tokio::try_join!(out, err).map_err(|e| e.to_string())?;
                out.and(err).map_err(|e| e.to_string())
            }).await;
            match captures {
                Ok(Ok(())) => {}
                Ok(Err(e)) => failure = Some(format!("Output capture failed: {e}")),
                Err(_) => failure = Some("Output drain did not finish; output may be incomplete".into()),
            }
            let mut p = running.progress.lock();
            match result {
                Some(Ok(status)) => {
                    p.exit_code = status.code();
                    if state == State::Completed && !status.success() { state = State::Failed; }
                }
                Some(Err(e)) => failure = Some(format!("Process wait failed: {e}")),
                None => {}
            }
            p.state = if failure.is_some() { State::Failed } else { state };
            p.error = failure;
        });
        let monitored = job.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = monitored.finished.cancelled() => {}
                _ = tokio::time::sleep_until(monitored.started_at + HEALTH_AFTER) => {
                    let notice = {
                        let mut progress = monitored.progress.lock();
                        health_notice(&monitored, &mut progress, Instant::now())
                    };
                    if let Some(notice) = notice { eprintln!("{notice}"); }
                }
            }
        });
        Ok(job)
    }

    fn get(&self, session: &str, id: &str) -> Result<Arc<Job>, String> {
        self.jobs.lock().iter().find(|j| j.session == session && j.id == id).cloned()
            .ok_or_else(|| format!("Background task '{id}' not found in this session (records expire on exit or eviction)."))
    }
    fn list(&self, session: &str) -> Value {
        json!(self.jobs.lock().iter().filter(|j| j.session == session)
            .map(|j| j.snapshot(false)).collect::<Vec<_>>())
    }
    async fn stop(&self, session: &str, id: &str) -> Result<Value, String> {
        let job = self.get(session, id)?;
        Self::request_stop(&job);
        job.finished.cancelled().await;
        Ok(job.snapshot(true))
    }
    fn request_stop(job: &Job) {
        let mut p = job.progress.lock();
        if p.state == State::Running {
            p.state = State::Stopping;
            job.cancel.cancel();
        }
    }
    pub async fn shutdown(&self, session: Option<&str>) {
        let jobs: Vec<_> = self.jobs.lock().iter()
            .filter(|j| session.is_none_or(|s| j.session == s)).cloned().collect();
        for job in &jobs { Self::request_stop(job); }
        for job in jobs { job.finished.cancelled().await; }
        if session.is_none() { self.jobs.lock().clear(); }
    }
    pub fn notices(&self, session: &str) -> Vec<String> {
        self.notices_at(session, Instant::now())
    }
    fn notices_at(&self, session: &str, now: Instant) -> Vec<String> {
        self.jobs.lock().iter().filter(|j| j.session == session).filter_map(|j| {
            let mut p = j.progress.lock();
            if let Some(notice) = health_notice(j, &mut p, now) { return Some(notice); }
            if !j.finished.is_cancelled() { return None; }
            if p.notified { return None; }
            p.notified = true;
            let label = serde_json::to_string(&j.description).unwrap();
            Some(format!("Background task {}: {:?} — {}. /tasks output {}", j.id, p.state, label, j.id))
        }).collect()
    }
    /// ctrlc's handler runs on a dedicated thread. process::exit skips destructors,
    /// so kill active groups and unlink archives before taking that existing path.
    pub fn emergency_shutdown(&self) {
        for job in self.jobs.lock().iter() {
            if let Err(error) = job.group.kill() {
                eprintln!("Could not terminate background task {}: {error}", job.id);
            }
            for path in &job.paths { let _ = std::fs::remove_file(path); }
        }
    }
}

// Marks an unexpected worker unwind as failed as well as waking waiters.
struct Completion(Arc<Job>);
impl Drop for Completion {
    fn drop(&mut self) {
        let _ = self.0.group.kill();
        let mut p = self.0.progress.lock();
        if matches!(p.state, State::Running | State::Stopping) {
            p.state = State::Failed;
            p.error = Some("Background worker interrupted; process outcome unknown".into());
        }
        p.finished_at = Some(Instant::now());
        self.0.finished.cancel();
    }
}
struct ProcessGroup(Mutex<Option<u32>>);
impl ProcessGroup {
    fn kill(&self) -> Result<(), nix::errno::Errno> {
        let mut group = self.0.lock();
        #[cfg(unix)]
        if let Some(pid) = *group {
            match nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGKILL) {
                Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
                Err(error) => return Err(error),
            }
        }
        *group = None;
        Ok(())
    }
}
impl Drop for ProcessGroup { fn drop(&mut self) { let _ = self.kill(); } }

async fn drain(mut pipe: impl AsyncRead + Unpin, file: std::fs::File, job: Arc<Job>, stderr: bool) -> std::io::Result<()> {
    let mut file = tokio::fs::File::from_std(file);
    let mut saved = 0;
    let mut bytes = [0u8; 8192];
    loop {
        let n = pipe.read(&mut bytes).await?;
        if n == 0 { break; }
        {
            let mut p = job.progress.lock();
            let output = if stderr { &mut p.stderr } else { &mut p.stdout };
            output.push(&bytes[..n]);
            p.last_output_at = Some(Instant::now());
        }
        let count = n.min(ARCHIVE_BYTES.saturating_sub(saved));
        file.write_all(&bytes[..count]).await?;
        saved += count;
    }
    file.flush().await
}

/// MyCLI opts into this wrapper at registration; SDK and simple-tier Bash stay
/// unchanged. The runner drops execute() on cancellation. ToolContext's
/// extensions do not carry a cancellation token, so ownership is enforced by a
/// drop guard until the result hands the registered task back to the caller.
pub struct AutoBackgroundBash;

#[async_trait]
impl Tool for AutoBackgroundBash {
    fn name(&self) -> &str { "Bash" }
    fn description(&self) -> &str {
        "Execute a shell command once. Return normal output if it finishes within 60 seconds; otherwise return its background task ID and partial output. Use BackgroundOutput to check health and actual exit status, or BackgroundStop to stop it; do not rerun the command. Default total timeout is 1 hour; explicit timeout is milliseconds (max 600000), including time after handoff. Cancellation before handoff stops the process group. At most 4 commands may run at once. Completed foreground cd commands persist cwd; handed-off jobs do not change foreground cwd."
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Execute }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }
    fn input_schema(&self) -> Value {
        let mut schema = cersei_tools::bash::BashTool.input_schema();
        schema["properties"]["timeout"]["description"] = json!("Total timeout in milliseconds, including after handoff (default 3600000; explicit values capped at 600000). Values at or below 60000 expire in the foreground.");
        schema
    }
    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        execute_bash(&MANAGER, input, ctx, BASH_HANDOFF).await
    }
}

struct ForegroundJob(Option<Arc<Job>>);
impl Drop for ForegroundJob {
    fn drop(&mut self) {
        if let Some(job) = self.0.take() {
            Manager::request_stop(&job);
            // Kill synchronously even if the runtime is being torn down. The
            // manager's worker still owns the child and reaps/drains it.
            let _ = job.group.kill();
        }
    }
}

async fn execute_bash(manager: &Manager, input: Value, ctx: &ToolContext, handoff: Duration) -> ToolResult {
    #[derive(Deserialize)]
    struct Input { command: String, timeout: Option<u64> }
    let args: Input = match serde_json::from_value(input.clone()) {
        Ok(args) => args,
        Err(error) => return ToolResult::error(format!("Invalid input: {error}")),
    };
    // Preserve the SDK's zero-timeout behavior without weakening Start's
    // positive-timeout validation. This path cannot reach a handoff.
    if args.timeout == Some(0) {
        return cersei_tools::bash::BashTool.execute(input, ctx).await;
    }
    let job = match manager.start(Start {
        command: args.command.clone(),
        description: args.command.trim().chars().take(256).collect(),
        timeout: args.timeout.map(|ms| ms.min(600_000)),
    }, ctx) {
        Ok(job) => job,
        Err(error) => return ToolResult::error(error),
    };
    let mut foreground = ForegroundJob(Some(job.clone()));
    job.progress.lock().notified = true; // no background completion notice for a normal Bash result
    if Duration::from_millis(job.timeout) <= handoff {
        // The explicit deadline belongs to the worker. Wait for cleanup rather
        // than handing off a command whose shorter timeout has already expired.
        job.finished.cancelled().await;
    } else {
        tokio::select! {
            biased;
            _ = job.finished.cancelled() => {}
            _ = tokio::time::sleep_until(job.started_at + handoff) => {}
        }
    }
    let result = if job.finished.is_cancelled() {
        // Match SDK Bash's existing cwd persistence for completed foreground
        // calls. Detached jobs never overwrite a later foreground cwd.
        let completed = {
            let p = job.progress.lock();
            matches!(p.state, State::Completed | State::Failed) && p.error.is_none()
        };
        if completed {
            if let Some(dir) = args.command.trim().strip_prefix("cd ") {
                let new_cwd = job.cwd.join(dir.trim());
                if new_cwd.exists() { session_shell_state(&ctx.session_id).lock().cwd = Some(new_cwd); }
            }
        }
        bash_result(&job)
    } else {
        job.progress.lock().notified = false;
        let mut value = job.snapshot(true);
        value["background"] = json!(true);
        value["instruction"] = json!(format!(
            "Command is tracked as task {} and has not been restarted. Use BackgroundOutput with this id to check output, health, and actual exit status; use BackgroundStop with this id to terminate it. Running does not mean success. Silence alone does not establish a hang.", job.id));
        ToolResult::success(serde_json::to_string_pretty(&value).unwrap()).with_metadata(value)
    };
    // No await between disarming and returning: cancellation of the pending
    // foreground future always retains the guard and terminates the group.
    foreground.0 = None;
    result
}

fn bash_result(job: &Job) -> ToolResult {
    let mut value = job.snapshot(true);
    value["output_bytes"] = json!(value["stdout_bytes"].as_u64().unwrap() + value["stderr_bytes"].as_u64().unwrap());
    let content = format!("{}{}{}", value["stdout"].as_str().unwrap(),
        if value["stderr_bytes"].as_u64().unwrap() > 0 { "\n" } else { "" }, value["stderr"].as_str().unwrap());
    let p = job.progress.lock();
    let result = match p.state {
        State::Completed if content.is_empty() => ToolResult::success("(Bash completed with no output)"),
        State::Completed => ToolResult::success(content),
        State::TimedOut => ToolResult::error(format!("Command timed out after {}ms\n{content}", job.timeout)),
        State::Failed => ToolResult::error(match &p.error {
            Some(error) => format!("Failed to execute: {error}\n{content}"),
            None => format!("Exit code {}\n{content}", p.exit_code.unwrap_or(-1)),
        }),
        _ => ToolResult::error(format!("Command stopped\n{content}")),
    };
    result.with_metadata(value)
}

#[derive(Clone, Copy)]
enum Action { Start, List, Output, Stop }
struct BackgroundTool(Action);
pub fn tools() -> Vec<Box<dyn Tool>> {
    [Action::Start, Action::List, Action::Output, Action::Stop].into_iter()
        .map(|a| Box::new(BackgroundTool(a)) as Box<dyn Tool>).collect()
}
#[async_trait]
impl Tool for BackgroundTool {
    fn name(&self) -> &str {
        match self.0 { Action::Start => "BackgroundStart", Action::List => "BackgroundList", Action::Output => "BackgroundOutput", Action::Stop => "BackgroundStop" }
    }
    fn description(&self) -> &str {
        match self.0 {
            Action::Start => "Start a noninteractive shell command in the background and return its task ID immediately. Continue other work, then use BackgroundOutput to check incremental output and actual exit status. Maximum 4 running jobs; default timeout 1 hour, maximum 24 hours. Jobs stop when MyCLI exits, the session changes, or tools switch to simple. No subagent is launched. Cwd/environment are copied from this session's Bash state; changes in the job do not affect foreground commands.",
            Action::List => "List this session's background shell jobs and actual status. Records are in-memory and bounded to the latest 32 jobs across sessions; completed records can be evicted.",
            Action::Output => "Read a background job's current bounded stdout/stderr, exit status, archive paths, and health snapshot without waiting. Health includes running status, elapsed seconds, and seconds since output (since start if none); long_running becomes true at 300 seconds. Silence does not prove a hang and does not stop a job. Running is not success. Archives retain the first 1 MiB per stream and are deleted on eviction or CLI exit. Avoid repeated polling; do other work between checks.",
            Action::Stop => "Terminate a background command's process group and wait for exit and output cleanup. Returns the actual final status; an already finished job keeps its original result.",
        }
    }
    fn permission_level(&self) -> PermissionLevel {
        match self.0 { Action::Start | Action::Stop => PermissionLevel::Execute, _ => PermissionLevel::ReadOnly }
    }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }
    fn input_schema(&self) -> Value {
        match self.0 {
            Action::Start => json!({"type":"object", "additionalProperties":false, "properties":{
                "command":{"type":"string", "description":"Shell command to run", "minLength":1, "maxLength":65536},
                "description":{"type":"string", "description":"Short label for the task", "minLength":1, "maxLength":1024},
                "timeout":{"type":"integer", "description":"Timeout in milliseconds (default 3600000)", "minimum":1, "maximum":MAX_TIMEOUT}
            }, "required":["command","description"]}),
            Action::List => json!({"type":"object", "properties":{}, "additionalProperties":false}),
            _ => json!({"type":"object", "additionalProperties":false, "properties":{"id":{"type":"string"}}, "required":["id"]}),
        }
    }
    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        execute(&MANAGER, self.0, input, ctx).await
    }
}
async fn execute(manager: &Manager, action: Action, input: Value, ctx: &ToolContext) -> ToolResult {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Id { id: String }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    let result: Result<Value, String> = async {
        match action {
            Action::Start => {
                let start = serde_json::from_value(input).map_err(|e| format!("Invalid input: {e}"))?;
                Ok(manager.start(start, ctx)?.snapshot(false))
            }
            Action::List => {
                serde_json::from_value::<Empty>(input).map_err(|e| format!("Invalid input: {e}"))?;
                Ok(manager.list(&ctx.session_id))
            }
            Action::Output | Action::Stop => {
                let id: Id = serde_json::from_value(input).map_err(|e| format!("Invalid input: {e}"))?;
                if matches!(action, Action::Stop) { manager.stop(&ctx.session_id, &id.id).await }
                else { Ok(manager.get(&ctx.session_id, &id.id)?.snapshot(true)) }
            }
        }
    }.await;
    match result {
        Ok(value) => ToolResult::success(serde_json::to_string_pretty(&value).unwrap()).with_metadata(value),
        Err(error) => ToolResult::error(error),
    }
}

pub async fn slash(session: &str, args: &str) -> Result<String, String> {
    let parts: Vec<_> = args.split_whitespace().collect();
    let value = match parts.as_slice() {
        [] | ["list"] => MANAGER.list(session),
        ["output", id] => MANAGER.get(session, id)?.snapshot(true),
        ["stop", "all"] => { MANAGER.shutdown(Some(session)).await; MANAGER.list(session) }
        ["stop", id] => MANAGER.stop(session, id).await?,
        _ => return Err("Usage: /tasks [list | output <id> | stop <id> | stop all]".into()),
    };
    Ok(serde_json::to_string_pretty(&value).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> ToolContext {
        ToolContext { working_dir: "/tmp".into(), session_id: uuid::Uuid::new_v4().to_string(),
            permissions: Arc::new(cersei_tools::permissions::AllowAll),
            cost_tracker: Arc::new(cersei_tools::CostTracker::new()), mcp_manager: None,
            extensions: cersei_tools::Extensions::default() }
    }
    fn start(manager: &Manager, ctx: &ToolContext, command: &str, timeout: u64) -> Arc<Job> {
        manager.start(Start { command: command.into(), description: "test job".into(), timeout: Some(timeout) }, ctx).unwrap()
    }
    async fn finished(job: &Job) {
        tokio::time::timeout(Duration::from_secs(5), job.finished.cancelled()).await.unwrap();
    }
    async fn wait_output(job: &Job, predicate: impl Fn(&str) -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if predicate(&job.progress.lock().stdout.text()) { return; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
    }
    // The workspace does not enable Tokio's test-util feature. Inject short
    // handoff durations and explicit snapshot instants instead of waiting
    // 60/300 real seconds. FIFO gates let tests release jobs deterministically.
    fn gated_context() -> (tempfile::TempDir, ToolContext) {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("mkfifo").arg(dir.path().join("gate")).status().unwrap().success());
        let ctx = ToolContext { working_dir: dir.path().to_owned(), ..context() };
        (dir, ctx)
    }
    #[tokio::test]
    async fn bash_fast_completion_preserves_ordinary_results_and_exit_codes() {
        let manager = Manager::default();
        let ctx = context();
        for (command, content, code) in [
            ("printf hello; printf error >&2", "hello\nerror", 0),
            ("printf hello; exit 7", "Exit code 7\nhello", 7),
            ("true", "(Bash completed with no output)", 0),
        ] {
            let result = execute_bash(&manager, json!({"command":command}), &ctx, BASH_HANDOFF).await;
            assert_eq!(result.content, content);
            assert_eq!(result.is_error, code != 0);
            let metadata = result.metadata.unwrap();
            assert_eq!(metadata["exit_code"], code);
            assert!(metadata.get("background").is_none());
        }
        assert!(manager.notices(&ctx.session_id).is_empty());
        let dir = tempfile::tempdir().unwrap();
        let result = execute_bash(&manager, json!({"command":format!("cd {}", dir.path().display())}), &ctx, BASH_HANDOFF).await;
        assert!(!result.is_error);
        assert_eq!(session_shell_state(&ctx.session_id).lock().cwd.as_deref(), Some(dir.path()));
    }
    #[tokio::test]
    async fn bash_handoff_keeps_one_process_output_exit_status_and_session() {
        let manager = Manager::default();
        let (dir, ctx) = gated_context();
        let result = execute_bash(&manager,
            json!({"command":"printf x >> starts; printf ready; read answer < gate; printf done; exit 7"}),
            &ctx, Duration::from_millis(100)).await;
        assert!(!result.is_error);
        assert!(result.content.contains("BackgroundOutput") && result.content.contains("BackgroundStop"));
        let value = result.metadata.unwrap();
        assert_eq!(value["background"], true);
        assert_eq!(value["stdout"], "ready");
        assert_eq!(value["status"], "running");
        assert!(value["exit_code"].is_null());
        let id = value["id"].as_str().unwrap();
        assert!(manager.get(&context().session_id, id).is_err());
        let job = manager.get(&ctx.session_id, id).unwrap();
        assert!(!job.cancel.is_cancelled());
        assert_eq!(job.timeout, 3_600_000);
        std::fs::write(dir.path().join("gate"), "continue\n").unwrap();
        finished(&job).await;
        let value = job.snapshot(true);
        assert_eq!(value["exit_code"], 7);
        assert_eq!(value["stdout"], "readydone");
        assert_eq!(value["status"], "failed");
        assert_eq!(std::fs::read_to_string(dir.path().join("starts")).unwrap(), "x");
        assert_eq!(manager.notices(&ctx.session_id).len(), 1);
    }
    #[tokio::test]
    async fn bash_cancellation_before_handoff_kills_and_reaps_the_group() {
        let manager = Arc::new(Manager::default());
        let (_dir, ctx) = gated_context();
        let task = tokio::spawn({
            let manager = manager.clone();
            let ctx = ctx.clone();
            async move { execute_bash(&manager,
                json!({"command":"printf ready; read answer < gate"}), &ctx, BASH_HANDOFF).await }
        });
        let job = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(job) = manager.jobs.lock().front().cloned() { break job; }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        wait_output(&job, |s| s == "ready").await;
        task.abort(); // dropping execute(), as the runner does on turn cancellation
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(job.cancel.is_cancelled());
        assert!(job.group.0.lock().is_none());
        finished(&job).await;
        assert_eq!(job.snapshot(false)["status"], "stopped");
        assert!(manager.notices(&ctx.session_id).is_empty());
    }
    #[tokio::test]
    async fn bash_short_timeout_returns_partial_output_without_handoff() {
        let manager = Manager::default();
        let (_dir, ctx) = gated_context();
        let result = execute_bash(&manager,
            json!({"command":"printf ready; read answer < gate", "timeout":100}), &ctx, BASH_HANDOFF).await;
        assert!(result.is_error);
        assert!(result.content.contains("timed out after 100ms") && result.content.contains("ready"));
        let value = result.metadata.unwrap();
        assert_eq!(value["status"], "timed_out");
        assert!(value.get("background").is_none());
        let job = manager.get(&ctx.session_id, value["id"].as_str().unwrap()).unwrap();
        assert!(job.finished.is_cancelled() && job.group.0.lock().is_none());
    }
    #[tokio::test]
    async fn bash_explicit_timeout_remains_in_force_after_handoff() {
        let manager = Manager::default();
        let (_dir, ctx) = gated_context();
        let result = execute_bash(&manager,
            json!({"command":"printf ready; read answer < gate", "timeout":200}),
            &ctx, Duration::from_millis(20)).await;
        let value = result.metadata.unwrap();
        assert_eq!(value["background"], true);
        let job = manager.get(&ctx.session_id, value["id"].as_str().unwrap()).unwrap();
        finished(&job).await;
        assert_eq!(job.timeout, 200);
        assert_eq!(job.snapshot(false)["status"], "timed_out");
    }
    #[tokio::test]
    async fn health_at_300_seconds_reports_silence_without_stopping_job() {
        let manager = Manager::default();
        let (_dir, ctx) = gated_context();
        let job = start(&manager, &ctx, "printf ready; read answer < gate", 5_000);
        wait_output(&job, |s| s == "ready").await;
        // Explicit instants exercise the threshold without advancing OS time.
        job.progress.lock().last_output_at = Some(job.started_at + Duration::from_secs(10));
        let before = job.snapshot_at(false, job.started_at + Duration::from_secs(299));
        assert_eq!(before["health"]["long_running"], false);
        let value = job.snapshot_at(true, job.started_at + HEALTH_AFTER);
        assert_eq!(value["health"]["long_running"], true);
        assert_eq!(value["health"]["running"], true);
        assert_eq!(value["health"]["elapsed_seconds"], 300);
        assert_eq!(value["health"]["seconds_since_output"], 290);
        assert_eq!(value["health"]["output_observed"], true);
        assert!(!job.cancel.is_cancelled());
        job.progress.lock().last_output_at = None;
        assert_eq!(job.snapshot_at(false, job.started_at + HEALTH_AFTER)["health"]["seconds_since_output"], 300);
        manager.stop(&ctx.session_id, &job.id).await.unwrap();
        assert_eq!(job.snapshot(false)["health"]["running"], false);
    }
    #[tokio::test]
    async fn returns_immediately_and_allows_foreground_work_with_live_output() {
        let manager = Manager::default();
        let ctx = context();
        let job = start(&manager, &ctx, "printf ready; sleep 30", 60_000);
        wait_output(&job, |s| s == "ready").await;
        assert_eq!(job.progress.lock().state, State::Running);
        let foreground = cersei_tools::bash::BashTool.execute(json!({"command":"printf foreground"}), &ctx).await;
        assert_eq!(foreground.content, "foreground");
        assert_eq!(manager.get(&ctx.session_id, &job.id).unwrap().snapshot(true)["stdout"], "ready");
        let (first, second) = tokio::join!(manager.stop(&ctx.session_id, &job.id), manager.stop(&ctx.session_id, &job.id));
        let result = first.unwrap();
        assert_eq!(second.unwrap(), result);
        assert_eq!(result["status"], "stopped");
        assert_eq!(result["stdout"], "ready");
    }
    #[tokio::test]
    async fn successful_and_failed_commands_report_exit_and_both_streams() {
        let manager = Manager::default();
        let ctx = context();
        for code in [0, 7] {
            let job = start(&manager, &ctx, &format!("printf hello; printf error >&2; exit {code}"), 5_000);
            finished(&job).await;
            let result = job.snapshot(true);
            assert_eq!(result["exit_code"], code);
            assert_eq!(result["stdout"], "hello");
            assert_eq!(result["stderr"], "error");
            assert_eq!(result["status"], if code == 0 { "completed" } else { "failed" });
            assert_eq!(manager.stop(&ctx.session_id, &job.id).await.unwrap(), result);
        }
        assert_eq!(manager.notices(&ctx.session_id).len(), 2);
        assert!(manager.notices(&ctx.session_id).is_empty());
    }
    #[tokio::test]
    async fn timeout_and_stop_kill_descendants_and_prevent_later_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::default();
        let ctx = ToolContext { working_dir: dir.path().to_owned(), ..context() };
        for (index, timeout) in [(0, 200), (1, 60_000)] {
            let job = start(&manager, &ctx,
                &format!("(sleep 0.5; touch marker{index}) & printf '%s' $!; wait"), timeout);
            wait_output(&job, |s| !s.is_empty()).await;
            let descendant: i32 = job.progress.lock().stdout.text().parse().unwrap();
            if index == 1 { manager.stop(&ctx.session_id, &job.id).await.unwrap(); }
            finished(&job).await;
            assert_eq!(job.snapshot(false)["status"], if index == 0 { "timed_out" } else { "stopped" });
            // Linux may leave an orphan zombie until PID 1 reaps it. It must
            // never be a live sleeping process after stop has acknowledged exit.
            #[cfg(target_os = "linux")]
            if let Ok(stat) = std::fs::read_to_string(format!("/proc/{descendant}/stat")) {
                assert_eq!(stat.rsplit_once(") ").unwrap().1.chars().next(), Some('Z'));
            }
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(!dir.path().join("marker0").exists());
        assert!(!dir.path().join("marker1").exists());
    }
    #[tokio::test]
    async fn shell_exit_cleans_up_descendants_holding_pipes() {
        let manager = Manager::default();
        let job = start(&manager, &context(), "sleep 30 & printf done", 60_000);
        finished(&job).await;
        assert_eq!(job.snapshot(true)["status"], "completed");
        assert_eq!(job.snapshot(true)["stdout"], "done");
    }
    #[tokio::test]
    async fn output_and_archives_are_bounded_and_cleaned_up() {
        let manager = Manager::default();
        let job = start(&manager, &context(), "head -c 1200000 /dev/zero; head -c 1200000 /dev/zero >&2; printf END", 10_000);
        finished(&job).await;
        let value = job.snapshot(true);
        assert_eq!(value["status"], "completed");
        assert_eq!(value["stdout_bytes"], 1_200_003);
        assert_eq!(value["stderr_bytes"], 1_200_000);
        assert_eq!(value["archive_truncated"], true);
        assert!(value["stdout"].as_str().unwrap().len() <= output::MODEL_OUTPUT_BYTES);
        assert!(value["stdout"].as_str().unwrap().ends_with("END"));
        let paths = job.paths.each_ref().map(|p| p.to_path_buf());
        for path in &paths { assert_eq!(std::fs::metadata(path).unwrap().len(), ARCHIVE_BYTES as u64); }
        drop(job);
        manager.shutdown(None).await;
        for path in paths { assert!(!path.exists()); }
    }
    #[tokio::test]
    async fn sessions_are_isolated_and_stop_all_is_scoped() {
        let manager = Manager::default();
        let a = context();
        let b = context();
        let job_a = start(&manager, &a, "sleep 30", 60_000);
        let job_b = start(&manager, &b, "sleep 30", 60_000);
        assert!(manager.get(&b.session_id, &job_a.id).is_err());
        assert!(manager.stop(&b.session_id, &job_a.id).await.is_err());
        assert_eq!(manager.list(&a.session_id).as_array().unwrap().len(), 1);
        manager.shutdown(Some(&a.session_id)).await;
        assert!(job_a.finished.is_cancelled());
        assert_eq!(job_b.snapshot(false)["status"], "running");
        manager.shutdown(None).await;
        assert_eq!(job_b.snapshot(false)["status"], "stopped");
    }
    #[tokio::test]
    async fn inherits_shell_snapshot_without_mutating_foreground_state() {
        let manager = Manager::default();
        let dir = tempfile::tempdir().unwrap();
        let ctx = context();
        let shell = session_shell_state(&ctx.session_id);
        {
            let mut shell = shell.lock();
            shell.cwd = Some(dir.path().to_owned());
            shell.env_vars.insert("MYCLI_TEST_VALUE".into(), "snapshot".into());
        }
        let job = start(&manager, &ctx, "printf '%s' \"$MYCLI_TEST_VALUE\"; pwd; cd /", 5_000);
        finished(&job).await;
        let text = job.snapshot(true)["stdout"].as_str().unwrap().to_owned();
        assert!(text.starts_with("snapshot"));
        assert!(text.contains(dir.path().to_str().unwrap()));
        assert_eq!(shell.lock().cwd.as_deref(), Some(dir.path()));
    }
    #[tokio::test]
    async fn enforces_running_limit_and_evicts_only_finished_records() {
        let manager = Manager::default();
        let ctx = context();
        for _ in 0..MAX_RUNNING { start(&manager, &ctx, "sleep 30", 60_000); }
        assert!(manager.start(Start { command: "true".into(), description: "extra".into(), timeout: None }, &ctx).is_err());
        manager.shutdown(Some(&ctx.session_id)).await;
        let active = start(&manager, &ctx, "sleep 30", 60_000);
        for _ in 0..MAX_RECORDS + 1 {
            let job = start(&manager, &ctx, "true", 5_000);
            finished(&job).await;
        }
        assert_eq!(manager.jobs.lock().len(), MAX_RECORDS);
        assert!(manager.get(&ctx.session_id, &active.id).is_ok());
        manager.shutdown(None).await;
    }
    #[tokio::test]
    async fn invalid_arguments_and_spawn_failure_do_not_create_records() {
        let manager = Manager::default();
        let ctx = context();
        for value in [json!({}), json!({"command":"", "description":"x"}),
            json!({"command":"true", "description":"x", "timeout":0}),
            json!({"command":"true", "description":"x", "timeout":MAX_TIMEOUT + 1}),
            json!({"command":"true", "description":"x", "background":true})] {
            assert!(execute(&manager, Action::Start, value, &ctx).await.is_error);
        }
        let ctx = ToolContext { working_dir: "/nonexistent-mycli-directory".into(), ..ctx };
        assert!(execute(&manager, Action::Start, json!({"command":"true", "description":"x"}), &ctx).await.is_error);
        assert!(manager.jobs.lock().is_empty());
    }
    #[test]
    fn utf8_output_remains_bounded() {
        let mut output = Output::default();
        output.push("漢🦀".repeat(10000).as_bytes());
        assert!(output.text().len() <= output::MODEL_OUTPUT_BYTES);
        assert!(output.text().contains("truncated"));
    }
    #[tokio::test]
    async fn long_running_job_emits_one_health_notice_without_stopping() {
        let manager = Manager::default();
        let ctx = context();
        let job = start(&manager, &ctx, "sleep 30", 60_000);
        assert!(manager.notices_at(&ctx.session_id, job.started_at + Duration::from_secs(299)).is_empty());
        let notices = manager.notices_at(&ctx.session_id, job.started_at + Duration::from_secs(301));
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("still running after 301s"));
        assert!(notices[0].contains("silence alone does not prove a hang"));
        assert!(manager.notices_at(&ctx.session_id, job.started_at + Duration::from_secs(302)).is_empty());
        assert_eq!(job.snapshot(false)["status"], "running");
        manager.shutdown(None).await;
    }
}
