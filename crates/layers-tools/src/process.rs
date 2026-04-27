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
    traits::ProcessRunStore,
};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

static PROCESS_MANAGER: OnceLock<Arc<ProcessManager>> = OnceLock::new();

/// Initialise the global process manager with an optional persistence store.
/// Must be called before `process_manager()` or the manager starts without persistence.
pub fn init_process_manager(store: Option<Arc<dyn ProcessRunStore>>) {
    let _ = PROCESS_MANAGER.set(Arc::new(ProcessManager::with_store(store)));
}

pub fn process_manager() -> Arc<ProcessManager> {
    PROCESS_MANAGER
        .get_or_init(|| Arc::new(ProcessManager::new()))
        .clone()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

/// Wrapper abstracting over tokio Child and PTY child processes.
enum ProcessChild {
    Std(Child),
    Pty {
        child: std::sync::Mutex<Box<dyn portable_pty::Child + Send>>,
        writer: std::sync::Mutex<Option<Box<dyn std::io::Write + Send>>>,
    },
}

impl ProcessChild {
    async fn kill(&mut self) -> std::io::Result<()> {
        match self {
            Self::Std(c) => c.kill().await,
            Self::Pty { child, .. } => child.get_mut().unwrap().kill(),
        }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match self {
            Self::Std(c) => c.try_wait(),
            Self::Pty { child, .. } => match child.get_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    let code = status.exit_code() as i32;
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        Ok(Some(std::process::ExitStatus::from_raw(code)))
                    }
                }
                Ok(None) => Ok(None),
                Err(e) => Err(e),
            },
        }
    }

    #[allow(dead_code)]
    fn id(&self) -> Option<u32> {
        match self {
            Self::Std(c) => c.id(),
            Self::Pty { child, .. } => child.lock().unwrap().process_id(),
        }
    }
}

struct ManagedProcess {
    run: ProcessRun,
    command: String,
    pid: Option<u32>,
    pty: bool,
    exit_code: Option<i32>,
    child: ProcessChild,
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
    store: Option<Arc<dyn ProcessRunStore>>,
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
            store: None,
        }
    }

    #[must_use]
    pub fn with_store(store: Option<Arc<dyn ProcessRunStore>>) -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
            store,
        }
    }

    /// Persist a status update to the backing store (if configured).
    async fn persist_status(
        &self,
        id: &str,
        status: ProcessRunStatus,
        finished_at: chrono::DateTime<chrono::Utc>,
        summary: Option<&str>,
    ) {
        if let Some(ref store) = self.store {
            if let Err(e) = store.update_status(id, status, finished_at, summary).await {
                warn!(run_id = %id, error = %e, "failed to persist process run status");
            }
        }
    }

    pub async fn spawn(&self, request: SpawnRequest) -> Result<ProcessSnapshot> {
        let run_id = uuid::Uuid::new_v4().to_string();
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));

        if request.pty {
            return self.spawn_pty(request, run_id, stdout, stderr).await;
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

        let pid = child.id();
        let stdin = child.stdin.take();

        let mut child = ProcessChild::Std(child);

        if let ProcessChild::Std(ref mut c) = child {
            if let Some(out) = c.stdout.take() {
                let lines = stdout.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(out).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        lines.lock().await.push(line);
                    }
                });
            }

            if let Some(err) = c.stderr.take() {
                let lines = stderr.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(err).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        lines.lock().await.push(line);
                    }
                });
            }
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
        // Persist initial Running state to store.
        if let Some(ref store) = self.store {
            if let Err(e) = store.put(managed.run.clone()).await {
                warn!(run_id = %run_id, error = %e, "failed to persist process run on spawn");
            }
        }
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

    /// Spawn a process inside a PTY, collecting merged stdout+stderr into the
    /// shared `stdout` buffer (stderr remains empty for PTY processes since
    /// the PTY merges both streams).
    async fn spawn_pty(
        &self,
        request: SpawnRequest,
        run_id: String,
        stdout: Arc<Mutex<Vec<String>>>,
        stderr: Arc<Mutex<Vec<String>>>,
    ) -> Result<ProcessSnapshot> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| LayersError::Tool(format!("failed to open PTY: {e}")))?;

        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string());

        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-lc");
        cmd.arg(&request.command);
        cmd.env("OPENCLAW_SHELL", "exec");

        if let Some(dir) = &request.workdir {
            cmd.cwd(dir);
        }

        for (key, value) in &request.env {
            cmd.env(key.as_str(), value.as_str());
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| LayersError::Tool(format!("failed to spawn PTY child: {e}")))?;

        let pid = child.process_id();

        // Drop the slave side so the PTY closes when the child exits.
        drop(pair.slave);

        // Read from master in a background thread.
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| LayersError::Tool(format!("failed to clone PTY reader: {e}")))?;

        let lines_clone = stdout.clone();
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let buf_reader = BufReader::new(reader);
            for line in buf_reader.lines() {
                match line {
                    Ok(l) => {
                        let mut guard = lines_clone.blocking_lock();
                        guard.push(l);
                    }
                    Err(_) => break,
                }
            }
        });

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| LayersError::Tool(format!("failed to get PTY writer: {e}")))?;

        let writer = std::sync::Mutex::new(Some(writer));

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
            pty: true,
            exit_code: None,
            child: ProcessChild::Pty {
                child: std::sync::Mutex::new(child),
                writer,
            },
            stdin: None, // PTY uses writer for stdin
            stdout,
            stderr,
            status: ManagedProcessStatus::Running,
        };

        let snapshot = managed.snapshot().await;
        if let Some(ref store) = self.store {
            if let Err(e) = store.put(managed.run.clone()).await {
                warn!(run_id = %run_id, error = %e, "failed to persist PTY process run on spawn");
            }
        }
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
        self.persist_status(
            &process.run.id,
            ProcessRunStatus::Failed,
            process.run.finished_at.unwrap(),
            process.run.result_summary.as_deref(),
        )
        .await;
        Ok(())
    }

    pub async fn poll(&self, run_id: &str) -> Result<ProcessSnapshot> {
        let mut processes = self.processes.lock().await;
        let process = processes
            .get_mut(run_id)
            .ok_or_else(|| LayersError::Tool(format!("process not found: {run_id}")))?;
        let prev_status = process.status.clone();
        Self::refresh_locked(process).await;
        // Persist if status changed.
        if process.status != prev_status {
            self.persist_status(
                &process.run.id,
                process.run.status.clone(),
                process.run.finished_at.unwrap_or_else(Utc::now),
                process.run.result_summary.as_deref(),
            )
            .await;
        }
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

        match &mut process.child {
            ProcessChild::Pty { writer, .. } => {
                let guard = writer.get_mut().unwrap();
                let w = guard.as_mut().ok_or_else(|| {
                    LayersError::Tool(format!("pty writer unavailable: {run_id}"))
                })?;
                use std::io::Write;
                w.write_all(data.as_bytes())
                    .map_err(|e| LayersError::Tool(format!("pty write failed: {e}")))?;
                w.flush()
                    .map_err(|e| LayersError::Tool(format!("pty flush failed: {e}")))?;
                if eof {
                    *guard = None;
                }
            }
            ProcessChild::Std(_) => {
                let stdin = process
                    .stdin
                    .as_mut()
                    .ok_or_else(|| LayersError::Tool(format!("stdin unavailable: {run_id}")))?;
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
            }
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
            self.persist_status(
                &process.run.id,
                ProcessRunStatus::Cancelled,
                process.run.finished_at.unwrap(),
                process.run.result_summary.as_deref(),
            )
            .await;
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
