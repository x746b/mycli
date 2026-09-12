//! Bash tool: execute shell commands.

use super::*;
use serde::Deserialize;
use std::process::Stdio;

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str { "Bash" }

    fn description(&self) -> &str {
        "Execute a bash command and return its output. The working directory persists between commands."
    }

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Execute }
    fn category(&self) -> ToolCategory { ToolCategory::Shell }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds (max 600000)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        #[derive(Deserialize)]
        struct Input {
            command: String,
            timeout: Option<u64>,
        }

        let input: Input = match serde_json::from_value(input) {
            Ok(i) => i,
            Err(e) => return ToolResult::error(format!("Invalid input: {}", e)),
        };

        let shell_state = session_shell_state(&ctx.session_id);
        let (cwd, env_vars) = {
            let state = shell_state.lock();
            (
                state.cwd.clone().unwrap_or_else(|| ctx.working_dir.clone()),
                state.env_vars.clone(),
            )
        };

        let timeout_ms = input.timeout.unwrap_or(120_000).min(600_000);

        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", &input.command])
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);

        for (k, v) in &env_vars {
            cmd.env(k, v);
        }

        let mut stdout = match output::Capture::new() {
            Ok(c) => c, Err(e) => return ToolResult::error(format!("Output capture: {e}")),
        };
        let mut stderr = match output::Capture::new() {
            Ok(c) => c, Err(e) => return ToolResult::error(format!("Output capture: {e}")),
        };
        let mut child = match cmd.spawn() {
            Ok(c) => c, Err(e) => return ToolResult::error(format!("Failed to execute: {e}")),
        };
        // Kill the process group on completion, timeout, or cancellation, including
        // descendants holding the output pipes open.
        let _group = ProcessGroup(child.id());
        let out_pipe = child.stdout.take().unwrap();
        let err_pipe = child.stderr.take().unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            async { tokio::try_join!(child.wait(), stdout.drain(out_pipe), stderr.drain(err_pipe)) },
        ).await;
        let content = format!("{}{}{}", stdout.text(), if stderr.total > 0 { "\n" } else { "" }, stderr.text());
        let metadata = serde_json::json!({"output_files": [stdout.path, stderr.path],
            "output_bytes": stdout.total + stderr.total,
            "archive_truncated": stdout.total > output::ARCHIVE_BYTES as u64 || stderr.total > output::ARCHIVE_BYTES as u64});
        let result = match result {
            Ok(Ok((status, (), ()))) => {
                // Update shell state for cd commands
                if input.command.trim().starts_with("cd ") {
                    let dir = input.command.trim().strip_prefix("cd ").unwrap().trim();
                    let new_cwd = if dir.starts_with('/') {
                        PathBuf::from(dir)
                    } else {
                        cwd.join(dir)
                    };
                    if new_cwd.exists() {
                        shell_state.lock().cwd = Some(new_cwd);
                    }
                }

                if status.success() {
                    if content.is_empty() {
                        ToolResult::success("(Bash completed with no output)")
                    } else {
                        ToolResult::success(content)
                    }
                } else {
                    let code = status.code().unwrap_or(-1);
                    ToolResult::error(format!(
                        "Exit code {}\n{}",
                        code,
                        content
                    ))
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("Failed to execute: {}", e)),
            Err(_) => ToolResult::error(format!(
                "Command timed out after {}ms\n{}",
                timeout_ms, content
            )),
        };
        result.with_metadata(metadata)
    }
}

struct ProcessGroup(Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> ToolContext {
        ToolContext { working_dir: "/tmp".into(), session_id: uuid::Uuid::new_v4().to_string(),
            permissions: std::sync::Arc::new(permissions::AllowAll),
            cost_tracker: std::sync::Arc::new(CostTracker::new()), mcp_manager: None,
            extensions: Extensions::default() }
    }
    #[tokio::test]
    async fn captures_both_streams_and_nonzero_status() {
        let result = BashTool.execute(serde_json::json!({"command":"printf 'hello'; printf 'error' >&2; exit 7"}), &context()).await;
        assert!(result.is_error);
        assert!(result.content.contains("Exit code 7") && result.content.contains("hello\nerror"));
    }
    #[tokio::test]
    async fn timeout_returns_partial_output_and_stops_descendants() {
        let result = BashTool.execute(serde_json::json!({"command":"printf 'started'; sleep 20 & wait", "timeout": 100}), &context()).await;
        assert!(result.is_error && result.content.contains("timed out"));
        assert!(result.content.contains("started"));
    }
    #[tokio::test]
    async fn large_stdout_and_stderr_do_not_deadlock() {
        let result = BashTool.execute(serde_json::json!({"command":"head -c 1000000 /dev/zero; head -c 1000000 /dev/zero >&2", "timeout": 5000}), &context()).await;
        assert!(!result.is_error);
        assert!(result.content.len() < 34000);
        assert_eq!(result.metadata.unwrap()["output_bytes"], 2000000);
    }
}
