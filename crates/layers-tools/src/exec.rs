//! Shell execution tool with managed foreground/background execution.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Deserialize;

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};

use crate::process::{ManagedProcessStatus, SpawnRequest, process_manager};

#[derive(Debug, Deserialize)]
struct ExecParams {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    #[serde(rename = "yieldMs", default)]
    yield_ms: Option<u64>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    background: Option<bool>,
    #[serde(default)]
    pty: Option<bool>,
}

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
        "Execute a shell command. Supports direct completion, managed background execution, timeouts, and yieldMs auto-backgrounding."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "The shell command to execute"},
                "workdir": {"type": "string", "description": "Working directory for the command"},
                "env": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Environment variables to set"
                },
                "yieldMs": {"type": "integer", "description": "How long to wait before returning a background run id"},
                "timeout": {"type": "integer", "description": "Timeout in seconds (default: 1800)"},
                "background": {"type": "boolean", "description": "Run immediately in background mode"},
                "pty": {"type": "boolean", "description": "Request PTY mode for TTY-required programs"}
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: serde_json::Value, context: ToolContext) -> Result<ToolOutput> {
        let params: ExecParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid exec params: {e}")))?;

        if params.pty.unwrap_or(false) {
            return Err(LayersError::Tool(
                "pty execution is not implemented yet in layers-tools::exec".into(),
            ));
        }

        let manager = process_manager();
        let run = manager
            .spawn(SpawnRequest {
                session_id: context.session_id,
                agent_id: context.agent_id,
                command: params.command,
                workdir: params.workdir,
                env: params.env.unwrap_or_default(),
                timeout: params.timeout.or(Some(1800)),
                pty: false,
            })
            .await?;

        if params.background.unwrap_or(false) {
            return Ok(ToolOutput::structured(serde_json::json!({
                "background": true,
                "run_id": run.run_id,
                "pid": run.pid,
                "status": run.status,
                "started_at": run.started_at
            })));
        }

        let yield_ms = params.yield_ms.unwrap_or(10_000);
        let deadline = Instant::now() + Duration::from_millis(yield_ms);

        loop {
            let snapshot = manager.poll(&run.run_id).await?;
            match snapshot.status {
                ManagedProcessStatus::Completed
                | ManagedProcessStatus::Failed
                | ManagedProcessStatus::Cancelled
                | ManagedProcessStatus::TimedOut => {
                    let logs = manager.log(&run.run_id, 0, usize::MAX).await?;
                    let stdout = logs
                        .get("stdout")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!([]));
                    let stderr = logs
                        .get("stderr")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!([]));
                    let exit_code = snapshot.exit_code.unwrap_or(match snapshot.status {
                        ManagedProcessStatus::Completed => 0,
                        _ => -1,
                    });
                    return Ok(ToolOutput::structured(serde_json::json!({
                        "background": false,
                        "run_id": snapshot.run_id,
                        "stdout": stdout,
                        "stderr": stderr,
                        "exit_code": exit_code,
                        "status": snapshot.status,
                        "timed_out": matches!(snapshot.status, ManagedProcessStatus::TimedOut)
                    })));
                }
                ManagedProcessStatus::Running => {
                    if Instant::now() >= deadline {
                        return Ok(ToolOutput::structured(serde_json::json!({
                            "background": true,
                            "run_id": snapshot.run_id,
                            "pid": snapshot.pid,
                            "status": snapshot.status,
                            "yield_ms": yield_ms,
                            "note": "command exceeded yieldMs and continues under process management"
                        })));
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layers_core::ToolContext;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "test-session".into(),
            agent_id: "test-agent".into(),
            channel: None,
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn exec_simple_command() {
        let tool = ExecTool::new();
        let result = tool
            .execute(serde_json::json!({ "command": "echo hello" }), test_ctx())
            .await
            .unwrap();
        assert!(result.content.contains("hello"));
        assert!(result.content.contains("\"background\":false"));
    }

    #[tokio::test]
    async fn exec_background_mode_returns_run_id() {
        let tool = ExecTool::new();
        let result = tool
            .execute(
                serde_json::json!({ "command": "sleep 0.2", "background": true }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(result.content.contains("run_id"));
        assert!(result.content.contains("\"background\":true"));
    }

    #[tokio::test]
    async fn exec_yield_ms_auto_backgrounds() {
        let tool = ExecTool::new();
        let result = tool
            .execute(
                serde_json::json!({ "command": "sleep 0.2", "yieldMs": 10 }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(result.content.contains("run_id"));
        assert!(result.content.contains("yield_ms"));
    }

    #[tokio::test]
    async fn exec_pty_not_supported_yet() {
        let tool = ExecTool::new();
        let result = tool
            .execute(
                serde_json::json!({ "command": "echo hello", "pty": true }),
                test_ctx(),
            )
            .await;
        assert!(result.is_err());
    }
}
