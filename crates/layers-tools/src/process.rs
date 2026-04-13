//! Process execution tools and in-memory lifecycle management.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;
use tracing::warn;

use layers_core::{
    LayersError, ProcessRun, ProcessRunStatus, Result, Tool, ToolContext, ToolOutput,
};

static PROCESS_MANAGER: OnceLock<Arc<ProcessManager>> = OnceLock::new();

pub fn process_manager() -> Arc<ProcessManager> {
    PROCESS_MANAGER
        .get_or_init(|| Arc::new(ProcessManager::new()))
        .clone()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedProcessStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl ManagedProcessStatus {
    fn to_core_status(&self) -> ProcessRunStatus {
        match self {
            Self::Running => ProcessRunStatus::Running,
            Self::Completed => ProcessRunStatus::Completed,
            Self::Failed | Self::TimedOut => ProcessRunStatus::Failed,
            Self::Cancelled => ProcessRunStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub run_id: String,
    pub status: ManagedProcessStatus,
    pub command: String,
    pub pid: Option<u32>,
    pub pty: bool,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub exit_code: Option<i32>,
    pub stdout_lines: usize,
    pub stderr_lines: usize,
}

struct ManagedProcess {
    run: ProcessRun,
    command: String,
    pid: Option<u32>,
    pty: bool,
    exit_code: Option<i32>,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
    status: ManagedProcessStatus,
}

impl ManagedProcess {
    async fn snapshot(&self) -> ProcessSnapshot {
        ProcessSnapshot {
            run_id: self.run.id.clone(),
            status: self.status.clone(),
            command: self.command.clone(),
            pid: self.pid,
            pty: self.pty,
            started_at: self.run.started_at,
            finished_at: self.run.finished_at,
            exit_code: self.exit_code,
            stdout_lines: self.stdout.lock().await.len(),
            stderr_lines: self.stderr.lock().await.len(),
        }
    }
}

pub struct ProcessManager {
    processes: Mutex<HashMap<String, ManagedProcess>>,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct SpawnRequest {
    pub session_id: String,
    pub agent_id: String,
    pub command: String,
    pub workdir: Option<String>,
    pub env: HashMap<String, String>,
    pub timeout: Option<u64>,
    pub pty: bool,
}

impl ProcessManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
        }
    }

    pub async fn spawn(&self, request: SpawnRequest) -> Result<ProcessSnapshot> {
        if request.pty {
            return Err(LayersError::Tool(
                "pty execution is not implemented yet for managed background processes".into(),
            ));
        }

        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string());

        let mut cmd = Command::new(&shell);
        cmd.arg("-lc").arg(&request.command);
        cmd.env("OPENCLAW_SHELL", "exec");
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(dir) = &request.workdir {
            cmd.current_dir(dir);
        }

        for (key, value) in request.env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| LayersError::Tool(format!("failed to spawn background process: {e}")))?;

        let run_id = uuid::Uuid::new_v4().to_string();
        let pid = child.id();
        let stdin = child.stdin.take();
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));

        if let Some(out) = child.stdout.take() {
            let lines = stdout.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(out).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    lines.lock().await.push(line);
                }
            });
        }

        if let Some(err) = child.stderr.take() {
            let lines = stderr.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(err).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    lines.lock().await.push(line);
                }
            });
        }

        let run = ProcessRun {
            id: run_id.clone(),
            parent_session_id: Some(request.session_id),
            agent_id: Some(request.agent_id),
            status: ProcessRunStatus::Running,
            started_at: Utc::now(),
            finished_at: None,
            result_summary: None,
        };

        let managed = ManagedProcess {
            run,
            command: request.command,
            pid,
            pty: request.pty,
            exit_code: None,
            child,
            stdin,
            stdout,
            stderr,
            status: ManagedProcessStatus::Running,
        };

        let snapshot = managed.snapshot().await;
        self.processes.lock().await.insert(run_id.clone(), managed);

        if let Some(timeout_secs) = request.timeout {
            let manager = process_manager();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
                let _ = manager.timeout(&run_id).await;
            });
        }

        Ok(snapshot)
    }

    async fn refresh_locked(process: &mut ManagedProcess) {
        if !matches!(process.status, ManagedProcessStatus::Running) {
            return;
        }

        match process.child.try_wait() {
            Ok(Some(status)) => {
                process.pid = None;
                process.exit_code = status.code();
                process.run.finished_at = Some(Utc::now());
                process.status = if status.success() {
                    ManagedProcessStatus::Completed
                } else {
                    ManagedProcessStatus::Failed
                };
                process.run.status = process.status.to_core_status();
                process.run.result_summary = Some(match process.status {
                    ManagedProcessStatus::Completed => "completed".to_string(),
                    ManagedProcessStatus::Failed => {
                        format!("failed with exit code {}", process.exit_code.unwrap_or(-1))
                    }
                    _ => "finished".to_string(),
                });
            }
            Ok(None) => {}
            Err(e) => {
                warn!(error = %e, run_id = %process.run.id, "failed to refresh process state")
            }
        }
    }

    async fn timeout(&self, run_id: &str) -> Result<()> {
        let mut processes = self.processes.lock().await;
        let Some(process) = processes.get_mut(run_id) else {
            return Ok(());
        };

        Self::refresh_locked(process).await;
        if !matches!(process.status, ManagedProcessStatus::Running) {
            return Ok(());
        }

        process
            .child
            .kill()
            .await
            .map_err(|e| LayersError::Tool(format!("failed to kill timed out process: {e}")))?;
        process.pid = None;
        process.status = ManagedProcessStatus::TimedOut;
        process.run.status = ProcessRunStatus::Failed;
        process.run.finished_at = Some(Utc::now());
        process.run.result_summary = Some("timed out".into());
        Ok(())
    }

    pub async fn poll(&self, run_id: &str) -> Result<ProcessSnapshot> {
        let mut processes = self.processes.lock().await;
        let process = processes
            .get_mut(run_id)
            .ok_or_else(|| LayersError::Tool(format!("process not found: {run_id}")))?;
        Self::refresh_locked(process).await;
        process.snapshot().await.pipe(Ok)
    }

    pub async fn list(&self) -> Result<Vec<ProcessSnapshot>> {
        let mut processes = self.processes.lock().await;
        let mut snapshots = Vec::with_capacity(processes.len());
        for process in processes.values_mut() {
            Self::refresh_locked(process).await;
            snapshots.push(process.snapshot().await);
        }
        Ok(snapshots)
    }

    pub async fn log(
        &self,
        run_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<serde_json::Value> {
        let mut processes = self.processes.lock().await;
        let process = processes
            .get_mut(run_id)
            .ok_or_else(|| LayersError::Tool(format!("process not found: {run_id}")))?;
        Self::refresh_locked(process).await;

        let stdout = process.stdout.lock().await;
        let stderr = process.stderr.lock().await;
        let stdout_slice: Vec<_> = stdout.iter().skip(offset).take(limit).cloned().collect();
        let stderr_slice: Vec<_> = stderr.iter().skip(offset).take(limit).cloned().collect();
        let stdout_total = stdout.len();
        let stderr_total = stderr.len();
        drop(stdout);
        drop(stderr);

        Ok(serde_json::json!({
            "run": process.snapshot().await,
            "stdout": stdout_slice,
            "stderr": stderr_slice,
            "offset": offset,
            "limit": limit,
            "stdout_total": stdout_total,
            "stderr_total": stderr_total
        }))
    }

    pub async fn write(&self, run_id: &str, data: &str, eof: bool) -> Result<ProcessSnapshot> {
        let mut processes = self.processes.lock().await;
        let process = processes
            .get_mut(run_id)
            .ok_or_else(|| LayersError::Tool(format!("process not found: {run_id}")))?;
        Self::refresh_locked(process).await;

        if !matches!(process.status, ManagedProcessStatus::Running) {
            return Err(LayersError::Tool(format!(
                "process is not running: {run_id}"
            )));
        }

        let stdin = process
            .stdin
            .as_mut()
            .ok_or_else(|| LayersError::Tool(format!("stdin unavailable for process: {run_id}")))?;
        stdin
            .write_all(data.as_bytes())
            .await
            .map_err(|e| LayersError::Tool(format!("stdin write failed: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| LayersError::Tool(format!("stdin flush failed: {e}")))?;

        if eof {
            process.stdin = None;
        }

        process.snapshot().await.pipe(Ok)
    }

    pub async fn kill(&self, run_id: &str) -> Result<ProcessSnapshot> {
        let mut processes = self.processes.lock().await;
        let process = processes
            .get_mut(run_id)
            .ok_or_else(|| LayersError::Tool(format!("process not found: {run_id}")))?;
        Self::refresh_locked(process).await;

        if matches!(process.status, ManagedProcessStatus::Running) {
            process
                .child
                .kill()
                .await
                .map_err(|e| LayersError::Tool(format!("kill failed: {e}")))?;
            process.pid = None;
            process.status = ManagedProcessStatus::Cancelled;
            process.run.status = ProcessRunStatus::Cancelled;
            process.run.finished_at = Some(Utc::now());
            process.run.result_summary = Some("killed".into());
        }

        process.snapshot().await.pipe(Ok)
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

#[derive(Debug, Deserialize)]
struct ProcessParams {
    action: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    eof: Option<bool>,
}

pub struct ProcessTool;

impl ProcessTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProcessTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for ProcessTool {
    fn name(&self) -> &str {
        "process"
    }

    fn description(&self) -> &str {
        "Manage background processes started by exec: list, poll, log, write, submit, or kill."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "poll", "log", "write", "submit", "kill"]},
                "session_id": {"type": "string", "description": "Managed process run id"},
                "data": {"type": "string", "description": "Input to write or submit"},
                "offset": {"type": "integer", "minimum": 0},
                "limit": {"type": "integer", "minimum": 1},
                "eof": {"type": "boolean", "description": "Close stdin after writing"}
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value, _context: ToolContext) -> Result<ToolOutput> {
        let params: ProcessParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid process params: {e}")))?;
        let manager = process_manager();

        let value =
            match params.action.as_str() {
                "list" => serde_json::json!({ "processes": manager.list().await? }),
                "poll" => {
                    let run_id = params.session_id.as_deref().ok_or_else(|| {
                        LayersError::Tool("process poll requires session_id".into())
                    })?;
                    serde_json::json!(manager.poll(run_id).await?)
                }
                "log" => {
                    let run_id = params.session_id.as_deref().ok_or_else(|| {
                        LayersError::Tool("process log requires session_id".into())
                    })?;
                    manager
                        .log(
                            run_id,
                            params.offset.unwrap_or(0),
                            params.limit.unwrap_or(200),
                        )
                        .await?
                }
                "write" => {
                    let run_id = params.session_id.as_deref().ok_or_else(|| {
                        LayersError::Tool("process write requires session_id".into())
                    })?;
                    let data = params.data.as_deref().unwrap_or_default();
                    serde_json::json!(
                        manager
                            .write(run_id, data, params.eof.unwrap_or(false))
                            .await?
                    )
                }
                "submit" => {
                    let run_id = params.session_id.as_deref().ok_or_else(|| {
                        LayersError::Tool("process submit requires session_id".into())
                    })?;
                    let mut data = params.data.unwrap_or_default();
                    if !data.ends_with('\n') {
                        data.push('\n');
                    }
                    serde_json::json!(
                        manager
                            .write(run_id, &data, params.eof.unwrap_or(false))
                            .await?
                    )
                }
                "kill" => {
                    let run_id = params.session_id.as_deref().ok_or_else(|| {
                        LayersError::Tool("process kill requires session_id".into())
                    })?;
                    serde_json::json!(manager.kill(run_id).await?)
                }
                other => {
                    return Err(LayersError::Tool(format!(
                        "unsupported process action: {other}"
                    )));
                }
            };

        Ok(ToolOutput::structured(value))
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
    async fn process_tool_can_list_spawned_processes() {
        let manager = process_manager();
        let snapshot = manager
            .spawn(SpawnRequest {
                session_id: "test-session".into(),
                agent_id: "test-agent".into(),
                command: "echo hello".into(),
                workdir: None,
                env: HashMap::new(),
                timeout: None,
                pty: false,
            })
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let tool = ProcessTool::new();
        let result = tool
            .execute(serde_json::json!({ "action": "list" }), test_ctx())
            .await
            .unwrap();
        assert!(result.content.contains(&snapshot.run_id));
    }

    #[tokio::test]
    async fn process_tool_poll_updates_status() {
        let manager = process_manager();
        let snapshot = manager
            .spawn(SpawnRequest {
                session_id: "test-session".into(),
                agent_id: "test-agent".into(),
                command: "echo hello".into(),
                workdir: None,
                env: HashMap::new(),
                timeout: None,
                pty: false,
            })
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let tool = ProcessTool::new();
        let result = tool
            .execute(
                serde_json::json!({ "action": "poll", "session_id": snapshot.run_id }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(result.content.contains("completed") || result.content.contains("failed"));
    }
}
