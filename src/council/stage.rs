use anyhow::{Context, Result};
use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::workspace_root;
use crate::types::{CouncilRunRecord, CouncilStageAttempt};
use crate::util::{compact, iso_now};

use super::artifacts::{output_quality_error, persist_run_state};
use super::convergence::first_non_empty_line;

pub struct StageSpec<'a> {
    pub stage: &'static str,
    pub model: &'static str,
    pub role: &'static str,
    pub command: &'a str,
}

pub enum StageOutcome {
    Succeeded(String),
    Failed { reason: String },
}

pub fn execute_stage(
    artifacts_dir: &Path,
    run: &mut CouncilRunRecord,
    stage_index: usize,
    spec: &StageSpec<'_>,
    prompt: &str,
    retry_limit: u32,
    timeout_secs: u64,
) -> Result<StageOutcome> {
    let prompt_path = std::path::PathBuf::from(&run.stages[stage_index].prompt_path);
    fs::write(&prompt_path, prompt)?;
    run.stages[stage_index].status = "running".to_string();
    run.updated_at = iso_now();
    persist_run_state(artifacts_dir, run)?;

    let max_attempts = retry_limit.max(1);
    for attempt in 1..=max_attempts {
        let stdout_path =
            artifacts_dir.join(format!("{}-attempt-{}.stdout.txt", spec.stage, attempt));
        let stderr_path =
            artifacts_dir.join(format!("{}-attempt-{}.stderr.txt", spec.stage, attempt));
        let started_at = iso_now();
        let started = Instant::now();

        let stdout_file = File::create(&stdout_path)?;
        let stderr_file = File::create(&stderr_path)?;
        let mut child = Command::new("/bin/sh")
            .arg("-lc")
            .arg(spec.command)
            .current_dir(workspace_root())
            .env("LAYERS_COUNCIL_STAGE", spec.stage)
            .env("LAYERS_COUNCIL_MODEL", spec.model)
            .env("LAYERS_COUNCIL_ROLE", spec.role)
            .env("LAYERS_COUNCIL_PROMPT_FILE", &prompt_path)
            .env("LAYERS_COUNCIL_OUTPUT_FILE", &stdout_path)
            .env("LAYERS_COUNCIL_ARTIFACT_DIR", artifacts_dir)
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .with_context(|| format!("failed to spawn {}", spec.stage))?;

        let pid = child.id();
        let mut attempt_record = CouncilStageAttempt {
            attempt,
            status: "running".to_string(),
            started_at,
            finished_at: None,
            duration_ms: None,
            pid: Some(pid),
            exit_code: None,
            stdout_path: stdout_path.display().to_string(),
            stderr_path: stderr_path.display().to_string(),
            error: None,
        };
        run.stages[stage_index]
            .attempts
            .push(attempt_record.clone());
        run.updated_at = iso_now();
        persist_run_state(artifacts_dir, run)?;

        let timeout = Duration::from_secs(timeout_secs.max(1));
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if started.elapsed() >= timeout {
                child.kill().ok();
                let _ = child.wait();
                break None;
            }
            thread::sleep(Duration::from_millis(100));
        };

        let finished_at = iso_now();
        let duration_ms = started.elapsed().as_millis() as u64;
        let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();

        let quality_error = output_quality_error(spec.stage, &stdout);
        let (attempt_status, exit_code, error) = match status {
            Some(exit) if exit.success() && quality_error.is_none() => {
                ("succeeded".to_string(), exit.code(), None)
            }
            Some(exit) if exit.success() => ("stalled".to_string(), exit.code(), quality_error),
            Some(exit) => (
                "failed".to_string(),
                exit.code(),
                Some(compact(&stderr, 240)),
            ),
            None => (
                "timed_out".to_string(),
                None,
                Some(format!("stage exceeded {} seconds", timeout_secs.max(1))),
            ),
        };

        attempt_record.status = attempt_status.clone();
        attempt_record.finished_at = Some(finished_at);
        attempt_record.duration_ms = Some(duration_ms);
        attempt_record.exit_code = exit_code;
        attempt_record.error = error.clone();
        if let Some(slot) = run.stages[stage_index].attempts.last_mut() {
            *slot = attempt_record;
        }

        if attempt_status == "succeeded" {
            run.stages[stage_index].status = "succeeded".to_string();
            run.stages[stage_index].output_path = stdout_path.display().to_string();
            run.stages[stage_index].summary = compact(first_non_empty_line(&stdout), 180);
            run.updated_at = iso_now();
            persist_run_state(artifacts_dir, run)?;
            return Ok(StageOutcome::Succeeded(stdout));
        }

        run.stages[stage_index].status = if attempt < max_attempts {
            "retrying".to_string()
        } else {
            "failed".to_string()
        };
        run.stages[stage_index].summary = compact(error.as_deref().unwrap_or("stage failed"), 180);
        run.updated_at = iso_now();
        persist_run_state(artifacts_dir, run)?;
    }

    let terminal_reason = run.stages[stage_index]
        .attempts
        .last()
        .map(|attempt| match attempt.status.as_str() {
            "timed_out" => "stage_timed_out",
            _ => "retries_exhausted",
        })
        .unwrap_or("stage_failed")
        .to_string();
    run.status = "failed".to_string();
    run.status_reason = terminal_reason.clone();
    run.updated_at = iso_now();
    persist_run_state(artifacts_dir, run)?;
    Ok(StageOutcome::Failed {
        reason: terminal_reason,
    })
}
