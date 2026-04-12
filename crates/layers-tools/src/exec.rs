//! Shell execution tool: run commands directly via `tokio::process::Command`.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;
use tracing::{debug, warn};

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ExecParams {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    background: Option<bool>,
    #[serde(default)]
    pty: Option<bool>,
}

// ---------------------------------------------------------------------------
// Tool
// ---------------------------------------------------------------------------

/// Shell execution tool. Runs commands directly with no sandbox or approval.
pub struct ExecTool;

impl ExecTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExecTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Execute a shell command. Returns stdout, stderr, and exit code. \
         Supports background execution, working directory, environment variables, and timeouts."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for the command"
                },
                "env": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Environment variables to set"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (kills process on expiry)"
                },
                "background": {
                    "type": "boolean",
                    "description": "Run in background and return process ID"
                },
                "pty": {
                    "type": "boolean",
                    "description": "Allocate a pseudo-terminal (best-effort)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: ExecParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid exec params: {e}")))?;

        let _ = params.pty; // PTY allocation is best-effort, not implemented here.

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&params.command);

        if let Some(ref dir) = params.workdir {
            cmd.current_dir(dir);
        }

        if let Some(ref env_vars) = params.env {
            for (key, val) in env_vars {
                cmd.env(key, val);
            }
        }

        // Background mode: spawn and return PID.
        if params.background.unwrap_or(false) {
            let child = cmd
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| LayersError::Tool(format!("failed to spawn background process: {e}")))?;

            let pid = child.id().unwrap_or(0);
            debug!(pid, command = %params.command, "spawned background process");

            return Ok(ToolOutput {
                content: serde_json::json!({
                    "pid": pid,
                    "background": true
                })
                .to_string(),
                attachments: Vec::new(),
                is_error: None,
            });
        }

        // Foreground mode: run with optional timeout.
        let timeout_duration = Duration::from_secs(params.timeout.unwrap_or(120));

        let result = tokio::time::timeout(timeout_duration, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                let content = serde_json::json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": exit_code,
                });

                Ok(ToolOutput {
                    content: content.to_string(),
                    attachments: Vec::new(),
                    is_error: if exit_code != 0 { Some(true) } else { None },
                })
            }
            Ok(Err(e)) => Err(LayersError::Tool(format!("exec failed: {e}"))),
            Err(_) => {
                warn!(command = %params.command, "exec timed out after {timeout_duration:?}");
                Err(LayersError::Timeout(timeout_duration))
            }
        }
    }
}
