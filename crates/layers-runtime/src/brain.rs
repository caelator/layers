//! CLI brain dispatcher — spawns coding agent CLIs and streams their output.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};

use tokio::sync::{RwLock, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info};

use layers_core::config::BrainConfig;
use layers_core::types::BrainEvent;

/// Dispatches prompts to CLI-based AI brains.
pub struct BrainDispatcher {
    brains: HashMap<String, BrainConfig>,
    sessions: Arc<RwLock<HashMap<String, String>>>,
    workdir: String,
}

impl BrainDispatcher {
    pub fn new(brains: HashMap<String, BrainConfig>, workdir: String) -> Self {
        info!(brain_count = brains.len(), workdir = %workdir, "brain dispatcher initialized");
        Self {
            brains,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            workdir,
        }
    }

    /// List available brain names.
    pub fn available_brains(&self) -> Vec<String> {
        let mut names: Vec<_> = self.brains.keys().cloned().collect();
        names.sort();
        names
    }

    /// Dispatch a prompt to a brain, returning a stream of events.
    pub async fn dispatch(
        &self,
        brain_name: &str,
        prompt: &str,
        session_id: Option<&str>,
    ) -> Result<ReceiverStream<BrainEvent>, BrainError> {
        let config = self
            .brains
            .get(brain_name)
            .ok_or_else(|| BrainError::NotFound(brain_name.to_string()))?
            .clone();

        let (tx, rx) = mpsc::channel(256);
        let prompt = prompt.to_string();
        let session_id = session_id.map(String::from);
        let sessions = Arc::clone(&self.sessions);
        let workdir = self.workdir.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::run_brain(
                &config,
                &prompt,
                session_id.as_deref(),
                &sessions,
                &workdir,
                &tx,
            )
            .await
            {
                error!(error = %e, "brain execution failed");
                let _ = tx
                    .send(BrainEvent::Error {
                        message: e.to_string(),
                    })
                    .await;
            }
        });

        Ok(ReceiverStream::new(rx))
    }

    async fn run_brain(
        config: &BrainConfig,
        prompt: &str,
        session_id: Option<&str>,
        sessions: &Arc<RwLock<HashMap<String, String>>>,
        workdir: &str,
        tx: &mpsc::Sender<BrainEvent>,
    ) -> Result<(), BrainError> {
        let mut args = config.args.clone();

        // Inject session continuity if configured
        if let (Some(session_arg), Some(sid)) = (&config.session_arg, session_id) {
            let guard = sessions.read().await;
            if let Some(cli_session) = guard.get(sid) {
                args.push(session_arg.clone());
                args.push(cli_session.clone());
            }
        }

        // Prompt is the final argument (unless pipe_stdin)
        if !config.pipe_stdin {
            args.push(prompt.to_string());
        }

        info!(cli = %config.cli, args_count = args.len(), "spawning brain");

        // For CLIs that output to stderr (read_stderr), merge stderr into stdout via shell
        let mut child = if config.read_stderr {
            let shell_cmd = format!(
                "{} {} 2>&1",
                config.cli,
                args.iter()
                    .map(|a| shlex::try_quote(a).unwrap_or(std::borrow::Cow::Borrowed(a)))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&shell_cmd)
                .current_dir(workdir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .env("TERM", "dumb")
                .spawn()
                .map_err(|e| BrainError::SpawnFailed(format!("{}: {}", config.cli, e)))?
        } else {
            tokio::process::Command::new(&config.cli)
                .args(&args)
                .current_dir(workdir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("TERM", "dumb")
                .spawn()
                .map_err(|e| BrainError::SpawnFailed(format!("{}: {}", config.cli, e)))?
        };

        // If pipe_stdin, write prompt to stdin instead of as an arg
        if config.pipe_stdin {
            // Remove the last arg (the prompt) since we'll pipe it
            // Actually the prompt was already added to args, but for pipe_stdin
            // we send it via stdin. The prompt arg is harmless if the CLI ignores trailing args.
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(prompt.as_bytes()).await;
                let _ = stdin.write_all(b"\n").await;
                drop(stdin); // close stdin to signal EOF
            }
        }

        let stdout_io = child.stdout.take();
        let stderr_io = child.stderr.take();

        // Drain stderr in background to prevent blocking
        let _drain_stderr = tokio::spawn(async move {
            if let Some(mut stderr) = stderr_io {
                let mut buf = Vec::new();
                let _ = tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut buf).await;
            }
        });

        let stdout = stdout_io.ok_or_else(|| BrainError::SpawnFailed("no stdout".into()))?;
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        match config.output.as_str() {
            "stream-json" => {
                let mut captured_session: Option<String> = None;
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.trim().is_empty() {
                        continue;
                    }

                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        // Capture session ID from any message that has one
                        for key in &["session_id", "sessionId"] {
                            if let Some(sid) = json.get(key).and_then(|v| v.as_str()) {
                                captured_session = Some(sid.to_string());
                            }
                        }

                        let msg_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        let content = match msg_type {
                            "assistant" => json
                                .get("message")
                                .and_then(|m| m.get("content"))
                                .and_then(|c| c.as_array())
                                .and_then(|arr| arr.first())
                                .and_then(|b| b.get("text"))
                                .and_then(|v| v.as_str()),
                            "content_block_delta" => json
                                .get("delta")
                                .and_then(|d| d.get("text"))
                                .and_then(|v| v.as_str()),
                            "result" => json.get("result").and_then(|v| v.as_str()),
                            "text" => json.get("text").and_then(|v| v.as_str()),
                            _ => None,
                        };

                        if let Some(text) = content {
                            if !text.is_empty() {
                                let _ = tx
                                    .send(BrainEvent::Token {
                                        content: text.to_string(),
                                    })
                                    .await;
                            }
                        }
                    } else {
                        let _ = tx.send(BrainEvent::Token { content: line }).await;
                    }
                }

                if let (Some(sid), Some(cli_sid)) =
                    (session_id.map(String::from), captured_session.clone())
                {
                    sessions.write().await.insert(sid, cli_sid);
                }
                let _ = tx
                    .send(BrainEvent::Done {
                        session_id: captured_session,
                    })
                    .await;
            }
            _ => {
                // Plain text — collect all output, filter noise, emit as tokens
                let mut full_output = String::new();
                while let Ok(Some(line)) = lines.next_line().await {
                    full_output.push_str(&line);
                    full_output.push('\n');
                }

                // For text-mode brains, try to extract just the meaningful response
                let response = Self::extract_response(&full_output, config.cli.as_str());
                if !response.is_empty() {
                    let _ = tx.send(BrainEvent::Token { content: response }).await;
                }
                let _ = tx.send(BrainEvent::Done { session_id: None }).await;
            }
        }

        // Wait for drain task
        let _ = _drain_stderr.await;

        let status = child.wait().await;
        if let Err(e) = &status {
            debug!(error = %e, "brain wait failed");
        }

        Ok(())
    }

    /// Extract the meaningful response from CLI output, filtering noise.
    fn extract_response(output: &str, cli: &str) -> String {
        let cli_name = std::path::Path::new(cli)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        match cli_name.as_str() {
            "codex" => {
                let lines: Vec<&str> = output.lines().collect();
                let mut response_lines = Vec::new();
                let mut past_preamble = false;
                let mut errors = Vec::new();
                for line in &lines {
                    let trimmed = line.trim();
                    if trimmed.starts_with("ERROR") {
                        errors.push(*line);
                        continue;
                    }
                    if trimmed.starts_with("Reading additional")
                        || trimmed.starts_with("Reading prompt")
                        || trimmed.starts_with("OpenAI Codex")
                        || trimmed.starts_with("workdir:")
                        || trimmed.starts_with("model:")
                        || trimmed.starts_with("provider:")
                        || trimmed.starts_with("approval:")
                        || trimmed.starts_with("sandbox:")
                        || trimmed.starts_with("reasoning")
                        || trimmed.starts_with("session id:")
                        || trimmed.starts_with("--------")
                        || trimmed.starts_with("user")
                        || trimmed.starts_with("rmcp")
                        || trimmed.is_empty()
                    {
                        if past_preamble {
                            response_lines.push(*line);
                        }
                        continue;
                    }
                    past_preamble = true;
                    response_lines.push(*line);
                }
                let response = response_lines.join("\n").trim().to_string();
                if response.is_empty() && !errors.is_empty() {
                    format!("Error: {}", errors.join("; "))
                } else {
                    response
                }
            }
            "gemini" => {
                // Filter out MCP status lines and other noise
                output
                    .lines()
                    .filter(|line| {
                        let trimmed = line.trim();
                        !trimmed.starts_with("MCP issues detected")
                            && !trimmed.starts_with("rmcp")
                            && !trimmed.is_empty()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string()
            }
            _ => output.trim().to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BrainError {
    #[error("brain not found: {0}")]
    NotFound(String),
    #[error("spawn failed: {0}")]
    SpawnFailed(String),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
}
