use std::fmt::Write;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};

use super::git;
use super::types::{ArtifactExtractMode, ProofRecord, ProofSpec};

pub fn run_proof(
    workspace_root: &Path,
    feature_id: &str,
    proof: &ProofSpec,
) -> Result<ProofRecord> {
    let started_at = Instant::now();
    let commit_sha = git::head_sha(workspace_root)?;
    let deadline = Instant::now() + Duration::from_secs(proof.timeout_secs);
    let output = run_command_with_deadline(
        || {
            Command::new("sh")
                .args(["-c", &proof.command])
                .current_dir(workspace_root)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        },
        deadline,
    )
    .with_context(|| format!("proof {} failed to execute", proof.id))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut passed = output.status.success();
    let exit_code = output.status.code().unwrap_or(-1);

    let (artifact, artifact_error) = match extract_artifact(proof.artifact_extract, &stdout) {
        Ok(value) => (value, None),
        Err(error) => {
            passed = false;
            if !stderr.is_empty() {
                stderr.push('\n');
            }
            let _ = write!(stderr, "artifact extraction failed: {error:#}");
            (None, Some(error.to_string()))
        }
    };

    Ok(ProofRecord {
        proof_id: proof.id.clone(),
        feature_id: feature_id.to_string(),
        category: proof.category,
        command: proof.command.clone(),
        passed,
        exit_code,
        commit_sha,
        timestamp: Utc::now(),
        duration_ms: started_at.elapsed().as_millis() as u64,
        stdout_hash: sha256(&stdout),
        stderr_hash: sha256(&stderr),
        stdout,
        stderr,
        artifact,
        artifact_error,
    })
}

fn extract_artifact(
    mode: Option<ArtifactExtractMode>,
    stdout: &str,
) -> Result<Option<serde_json::Value>> {
    let Some(mode) = mode else {
        return Ok(None);
    };

    let trimmed = stdout.trim();
    match mode {
        ArtifactExtractMode::Json => {
            if trimmed.is_empty() {
                anyhow::bail!("stdout is empty; cannot parse JSON artifact");
            }
            if let Ok(value) = serde_json::from_str(trimmed) {
                return Ok(Some(value));
            }

            let Some(line) = trimmed.lines().rev().find(|line| !line.trim().is_empty()) else {
                anyhow::bail!("stdout is empty; cannot parse JSON artifact");
            };
            let value = serde_json::from_str(line.trim())
                .context("failed to parse full stdout or last line as JSON")?;
            Ok(Some(value))
        }
        ArtifactExtractMode::LastLine => {
            let line = trimmed
                .lines()
                .rev()
                .find(|candidate| !candidate.trim().is_empty())
                .unwrap_or_default()
                .trim()
                .to_string();
            Ok(Some(serde_json::Value::String(line)))
        }
        ArtifactExtractMode::FullOutput => Ok(Some(serde_json::Value::String(stdout.to_string()))),
    }
}

fn run_command_with_deadline<F>(
    spawn: F,
    deadline: Instant,
) -> std::io::Result<std::process::Output>
where
    F: FnOnce() -> std::io::Result<Child>,
{
    let mut child = spawn()?;
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();

    loop {
        if let Some(ref mut out) = child.stdout {
            let mut chunk = [0_u8; 4096];
            loop {
                match out.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => stdout_buf.extend_from_slice(&chunk[..n]),
                    Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(ref error) if error.kind() == std::io::ErrorKind::TimedOut => break,
                    Err(error) => return Err(error),
                }
            }
        }

        if let Some(ref mut err) = child.stderr {
            let mut chunk = [0_u8; 4096];
            loop {
                match err.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => stderr_buf.extend_from_slice(&chunk[..n]),
                    Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(ref error) if error.kind() == std::io::ErrorKind::TimedOut => break,
                    Err(error) => return Err(error),
                }
            }
        }

        if let Some(status) = child.try_wait()? {
            if let Some(ref mut out) = child.stdout {
                let mut chunk = [0_u8; 4096];
                loop {
                    match out.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => stdout_buf.extend_from_slice(&chunk[..n]),
                        Err(ref error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                || error.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            break;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            if let Some(ref mut err) = child.stderr {
                let mut chunk = [0_u8; 4096];
                loop {
                    match err.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => stderr_buf.extend_from_slice(&chunk[..n]),
                        Err(ref error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                || error.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            break;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }

            return Ok(std::process::Output {
                status,
                stdout: stdout_buf,
                stderr: stderr_buf,
            });
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "proof command did not complete within deadline",
            ));
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

fn sha256(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!("sha256:{digest:x}")
}
