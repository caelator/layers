//! `layers monitor` - autonomous repo health monitor.
//!
//! Runs one monitoring cycle per invocation: git sync, build check, test check,
//! GitHub Actions CI check, stale council-run archival, and fix subagent spawning.
//!
//! Designed to be run from a cron job (one-shot per cycle) rather than as a daemon.
//! A lock file prevents concurrent instances.

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use serde::{Deserialize, Serialize};

use substrate::StorageSafety;

use crate::technician::data::{EscalationLifecycle, EscalationRecord};

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
pub enum MonitorArgs {
    /// Run one autonomous monitoring cycle then exit.
    Run {
        /// Override the list of repos to monitor (default: layers, openclaw-pm,
        /// research-radar, council).
        #[arg(long)]
        repos: Option<String>,
    },
    /// Print the current monitor lock status.
    Status,
    /// Print the critical findings log.
    Findings,
}

pub fn handle_monitor(args: &MonitorArgs) -> Result<()> {
    match args {
        MonitorArgs::Run { repos } => run_monitor_cycle(repos.as_ref()),
        MonitorArgs::Status => print_status(),
        MonitorArgs::Findings => print_findings(),
    }
}

// ─── Paths ────────────────────────────────────────────────────────────────────

/// Returns the layers root directory (~/.layers).
fn layers_root() -> PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join(".layers")
}

/// Returns the repos directory (~/Documents/GitHub).
fn repos_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Documents/GitHub")
}

fn lock_file() -> PathBuf {
    layers_root().join(".monitor.lock")
}

fn log_file() -> PathBuf {
    layers_root().join("monitor.log")
}

fn findings_file() -> PathBuf {
    layers_root().join(".critical-findings.md")
}

fn state_file() -> PathBuf {
    layers_root().join(".monitor-state")
}

fn fix_queue_file() -> PathBuf {
    layers_root().join(".fix-queue.jsonl")
}

fn council_runs_dir() -> PathBuf {
    layers_root()
        .join("../.memoryport/council-runs")
        // resolve the relative path
        .canonicalize()
        .unwrap_or_else(|_| layers_root().join(".memoryport/council-runs"))
}

// ─── Logging ─────────────────────────────────────────────────────────────────

fn log(msg: &str) {
    let ts = chrono::Local::now().format("%T");
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())
    {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
    eprintln!("[monitor {ts}] {msg}");
}

fn update_state(state: &str) -> anyhow::Result<()> {
    use substrate::DefaultStorage;
    use substrate::StorageSafety;
    let path = state_file();
    let content = format!("workdir:{state}\n");
    if fs::read_to_string(&path)
        .map(|c| c.replace("workdir:idle", &format!("workdir:{state}")))
        .is_ok()
    {
        DefaultStorage::atomic_write(&path, content.as_bytes())?;
    }
    Ok(())
}

// ─── Findings ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

fn record_finding(severity: Severity, repo: &str, msg: &str) {
    let findings_path = findings_file();
    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let marker = match severity {
        Severity::Critical => "critical",
        Severity::Warning => "warning",
        Severity::Info => "info",
    };
    let entry = format!("\n## [{ts}] {marker} | {repo}\n\n{msg}\n");
    if let Err(e) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&findings_path)
        .and_then(|mut f| f.write_all(entry.as_bytes()))
    {
        log(&format!("WARNING: could not write findings file: {e}"));
    }
    log(&format!("FINDING({severity:?}): {msg}"));
}

// ─── Lock management ──────────────────────────────────────────────────────────

/// Acquires an exclusive flock lock on the lock file.
/// Returns the locked file descriptor (kept open to hold the lock).
/// Exits the process if the lock is held by another process.
// SAFETY: flock is the only viable file-locking mechanism on BSD/macOS.
// The file descriptor is held for the duration of the process and only used
// for kernel-level advisory locking - no other unsafe behavior flows from this call.
#[allow(unsafe_code)]
fn acquire_lock() -> Result<File> {
    let lock_path = lock_file();
    fs::create_dir_all(lock_path.parent().unwrap())?;

    let mut file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&lock_path)
        .context("open lock file")?;

    // Try non-blocking exclusive lock
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        // Write our PID and start time
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        let info = format!(
            "pid:{}\nstarted:{}\nworkdir:idle\n",
            std::process::id(),
            now
        );
        file.set_len(0)?;
        file.write_all(info.as_bytes())?;
        Ok(file)
    } else {
        // Lock held by another process - that's fine, just exit
        log(&format!(
            "Lock held by another process (PID from lock file: {}), exiting",
            fs::read_to_string(&lock_path).unwrap_or_default()
        ));
        std::process::exit(0);
    }
}

// ─── Git operations ───────────────────────────────────────────────────────────

fn is_repo_clean(dir: &PathBuf) -> bool {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output();
    match output {
        Ok(o) => o.stdout.is_empty(),
        Err(_) => false,
    }
}

fn push_if_dirty(dir: &PathBuf, name: &str) -> Result<()> {
    if is_repo_clean(dir) {
        return Ok(());
    }

    log(&format!("Dirty: {name} - committing and pushing"));
    update_state(&format!("committing:{name}"))?;

    let status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir)
        .status()
        .context("git add")?;

    if !status.success() {
        log(&format!("git add failed in {name}: {status}"));
        return Ok(());
    }

    // Validate the staged diff before committing
    if let Err(reason) = validate_staged_diff(dir, name) {
        log(&format!("Diff validation FAILED for {name}: {reason}"));
        record_finding(
            Severity::Critical,
            name,
            &format!("Auto-commit blocked by diff validation: {reason}"),
        );
        // Reset staging area to avoid leaving bad state
        let _ = Command::new("git")
            .args(["reset", "HEAD", "--quiet"])
            .current_dir(dir)
            .status();
        return Ok(());
    }

    let msg = format!(
        "auto-commit by autonomous monitor {}\n",
        Utc::now().format("%Y-%m-%d")
    );
    let commit_status = Command::new("git")
        .args(["commit", "-m", &msg, "--quiet"])
        .current_dir(dir)
        .status();

    if commit_status.is_ok_and(|s| s.success()) {
        let push_status = Command::new("git")
            .args(["push", "origin", "main"])
            .current_dir(dir)
            .status();
        if push_status.is_ok_and(|s| s.success()) {
            log(&format!("Pushed: {name}"));
        } else {
            log(&format!("Push failed: {name}"));
        }
    }
    Ok(())
}

/// Validate the staged diff before committing.
///
/// Returns `Ok(())` if the diff passes all checks, or `Err(reason)` if it should be blocked.
/// Checks:
///   1. No secrets or credentials staged (.env, private keys, tokens)
///   2. No mass deletions (>50% of tracked files)
///   3. No excessively large diffs (>10k lines changed)
///   4. No binary blobs over 1 MB
fn validate_staged_diff(dir: &PathBuf, name: &str) -> std::result::Result<(), String> {
    // --- Check 1: Secrets / credential patterns in staged file names ---
    let diff_names = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git diff --name-only failed: {e}"))?;

    let staged_files = String::from_utf8_lossy(&diff_names.stdout);
    let secret_patterns = [
        ".env",
        ".pem",
        ".p12",
        ".pfx",
        "id_rsa",
        "id_ed25519",
        "credentials.json",
        "service-account",
        ".secret",
        "token.json",
    ];
    for file in staged_files.lines() {
        let lower = file.to_lowercase();
        for pat in &secret_patterns {
            if lower.ends_with(pat) || lower.contains(&format!("{pat}/")) {
                return Err(format!(
                    "potential secret staged: '{file}' matches pattern '{pat}'"
                ));
            }
        }
    }

    // Also scan diff content for high-entropy secret patterns
    let diff_content = Command::new("git")
        .args(["diff", "--cached", "-U0"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git diff --cached failed: {e}"))?;

    let diff_text = String::from_utf8_lossy(&diff_content.stdout);
    let secret_content_patterns = [
        "PRIVATE KEY",
        "aws_secret_access_key",
        "ghp_",       // GitHub personal access token
        "sk-",        // OpenAI / Anthropic API key prefix
        "sk-ant-",    // Anthropic key prefix
        "AKIA",       // AWS access key ID prefix
        "password =", // Hardcoded password
    ];
    for line in diff_text.lines() {
        // Only check added lines
        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        for pat in &secret_content_patterns {
            if line.contains(pat) {
                return Err(format!(
                    "potential secret in diff content: line matches pattern '{pat}'"
                ));
            }
        }
    }

    // --- Check 2: Mass deletions (>50% of tracked files) ---
    let tracked = Command::new("git")
        .args(["ls-files"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git ls-files failed: {e}"))?;

    let total_tracked = String::from_utf8_lossy(&tracked.stdout).lines().count();

    if total_tracked > 1 {
        let deleted = Command::new("git")
            .args(["diff", "--cached", "--diff-filter=D", "--name-only"])
            .current_dir(dir)
            .output()
            .map_err(|e| format!("git diff deletions failed: {e}"))?;

        let deleted_count = String::from_utf8_lossy(&deleted.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .count();

        if deleted_count > 0 {
            let pct = deleted_count * 100 / total_tracked;
            if pct > 50 {
                return Err(format!(
                    "mass deletion: {deleted_count}/{total_tracked} files ({pct}%) staged for removal"
                ));
            }
        }
    }

    // --- Check 3: Excessively large diff (>10k lines) ---
    let stat = Command::new("git")
        .args(["diff", "--cached", "--numstat"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git diff --numstat failed: {e}"))?;

    let mut total_added: usize = 0;
    let mut total_removed: usize = 0;
    let mut binary_files: Vec<String> = Vec::new();

    for line in String::from_utf8_lossy(&stat.stdout).lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            if parts[0] == "-" && parts[1] == "-" {
                // Binary file
                binary_files.push(parts[2].to_string());
            } else {
                total_added += parts[0].parse::<usize>().unwrap_or(0);
                total_removed += parts[1].parse::<usize>().unwrap_or(0);
            }
        }
    }

    let total_changed = total_added + total_removed;
    if total_changed > 10_000 {
        return Err(format!(
            "diff too large: {total_changed} lines changed (+{total_added} -{total_removed})"
        ));
    }

    // --- Check 4: Binary blobs over 1 MB ---
    for bin_file in &binary_files {
        let full_path = dir.join(bin_file);
        if let Ok(meta) = fs::metadata(&full_path) {
            if meta.len() > 1_000_000 {
                return Err(format!(
                    "binary blob too large: '{}' is {} bytes",
                    bin_file,
                    meta.len()
                ));
            }
        }
    }

    log(&format!(
        "Diff validation passed for {name}: +{total_added} -{total_removed}, {} files staged",
        staged_files.lines().count()
    ));
    Ok(())
}

fn check_remote_sync(dir: &PathBuf, name: &str) -> Result<()> {
    update_state(&format!("syncing:{name}"))?;

    // Fetch latest
    let fetch = Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(dir)
        .output();

    if !fetch.as_ref().is_ok_and(|o| o.status.success()) {
        log(&format!("git fetch failed for {name}"));
        return Ok(());
    }

    let local_sha = Command::new("git")
        .args(["rev-parse", "@"])
        .current_dir(dir)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let remote_sha = Command::new("git")
        .args(["rev-parse", "origin/main"])
        .current_dir(dir)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if local_sha != remote_sha && !remote_sha.is_empty() {
        log(&format!("{name} behind origin - rebasing"));
        update_state(&format!("rebasing:{name}"))?;
        let rebase = Command::new("git")
            .args(["pull", "--rebase", "origin", "main"])
            .current_dir(dir)
            .output();

        if rebase.as_ref().map_or(true, |o| !o.status.success()) {
            log(&format!(
                "Rebase conflict: {name} - recorded finding (awaiting escalation)"
            ));
            record_finding(
                Severity::Critical,
                name,
                &format!("Rebase conflict in {}", dir.display()),
            );
        } else {
            log(&format!("Rebased: {name}"));
        }
    }
    Ok(())
}

// ─── Build / test checks ──────────────────────────────────────────────────────

fn check_build_and_tests(dir: &PathBuf, name: &str) -> Result<()> {
    update_state(&format!("building:{name}"))?;
    log(&format!("Running cargo build: {name}"));

    let build = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(dir)
        .output();

    let build_ok = build.as_ref().ok().is_some_and(|o| o.status.success());

    if !build_ok {
        let errors = parse_build_errors(
            &build
                .as_ref()
                .ok()
                .map_or_else(Vec::new, |o| o.stderr.clone()),
        );
        log(&format!("Build errors in {name}: {} errors", errors.len()));
        record_finding(
            Severity::Warning,
            name,
            &format!("Build errors in {}: {} errors", dir.display(), errors.len()),
        );
        return Ok(());
    }

    update_state(&format!("testing:{name}"))?;
    log(&format!("Running cargo test: {name}"));

    let test = Command::new("cargo")
        .args(["test", "--", "--test-threads=1"])
        .current_dir(dir)
        .output();

    let test_ok = test.as_ref().ok().is_some_and(|o| o.status.success());

    if test_ok {
        log(&format!("Tests passed: {name}"));
    } else {
        let stdout = test
            .as_ref()
            .ok()
            .map_or_else(|| &[][..], |o| o.stdout.as_slice());
        let stderr = test
            .as_ref()
            .ok()
            .map_or_else(|| &[][..], |o| o.stderr.as_slice());
        let failed = parse_test_failures(stdout);
        let build_errors = parse_build_errors(stderr);
        if !failed.is_empty() {
            log(&format!(
                "Test failures in {name}: {} tests failed",
                failed.len()
            ));
            record_finding(
                Severity::Warning,
                name,
                &format!(
                    "Test failures in {}: {} tests failed",
                    dir.display(),
                    failed.len()
                ),
            );
        } else if !build_errors.is_empty() {
            log(&format!(
                "Test compilation errors in {name}: {} errors",
                build_errors.len()
            ));
            record_finding(
                Severity::Warning,
                name,
                &format!(
                    "Test compilation errors in {}: {} errors",
                    dir.display(),
                    build_errors.len()
                ),
            );
        } else {
            // Non-zero exit but no detected failures — cargo itself may have been killed or timed out
            log(&format!(
                "cargo test exited non-zero for {name} with no parsed errors"
            ));
        }
    }
    Ok(())
}

fn parse_build_errors(stderr: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .filter(|l| l.contains("error[E"))
        .map(std::string::ToString::to_string)
        .collect()
}

fn parse_test_failures(stdout: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(stdout);
    text.lines()
        .filter(|l| l.contains(" FAILED"))
        .map(std::string::ToString::to_string)
        .collect()
}

// ─── GitHub Actions CI ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GhRun {
    #[serde(rename = "status", skip)]
    #[allow(dead_code)]
    status: String,
    #[serde(rename = "conclusion")]
    conclusion: Option<String>,
    #[serde(rename = "name")]
    name: Option<String>,
}

fn check_ci_status(name: &str, dir: &Path) -> Result<()> {
    if !dir.join(".git").exists() {
        return Ok(());
    }

    update_state(&format!("checking:ci:{name}"))?;

    let output = Command::new("gh")
        .args([
            "run",
            "list",
            "--repo",
            &format!("caelator/{name}"),
            "--limit",
            "3",
            "--json",
            "status,conclusion,name",
        ])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Ok(()),
    };

    let runs: Vec<GhRun> = match serde_json::from_slice(&output) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    let Some(latest) = runs.first() else {
        return Ok(());
    };

    if latest.conclusion.as_deref() == Some("failure") {
        let run_name = latest.name.as_deref().unwrap_or("unknown");
        log(&format!("CI failure in caelator/{name}: {run_name}"));
        record_finding(
            Severity::Critical,
            name,
            &format!("GitHub Actions failure in caelator/{name}. Run: {run_name}"),
        );
    }

    Ok(())
}

// ─── Council run archival ─────────────────────────────────────────────────────

fn archive_stale_council_runs() -> Result<()> {
    let runs_dir = council_runs_dir();
    if !runs_dir.exists() {
        return Ok(());
    }

    let stale_cutoff = chrono::Utc::now() - chrono::Duration::days(7);

    for entry in fs::read_dir(&runs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if !name.starts_with("council-") {
            continue;
        }

        // Extract date from council-<date> format (first 8 chars after council-)
        let date_str = name
            .strip_prefix("council-")
            .unwrap_or("")
            .chars()
            .take(8)
            .collect::<String>();
        let run_date = chrono::NaiveDate::parse_from_str(&date_str, "%Y%m%d")
            .ok()
            .map(|d| {
                DateTime::<Utc>::from_naive_utc_and_offset(
                    d.and_hms_opt(0, 0, 0).unwrap_or_default(),
                    Utc,
                )
            });

        let is_stale = run_date.is_some_and(|d| d < stale_cutoff);

        if is_stale {
            let archived_dir = runs_dir.join("archived");
            fs::create_dir_all(&archived_dir)?;
            let dest = archived_dir.join(name);
            log(&format!("Archiving stale council run: {name}"));
            fs::rename(&path, &dest).ok();
        }
    }
    Ok(())
}

// ─── Fix subagent spawning ────────────────────────────────────────────────────

/// Resolve the openclaw CLI binary path.
///
/// Checks, in order: `OPENCLAW_CLI` env var, `which openclaw`, then common
/// install locations. Returns an error instead of silently using a path
/// that doesn't exist (the old ENOENT bug).
fn resolve_openclaw_cli() -> Result<String> {
    // 1. Explicit env override
    if let Ok(p) = std::env::var("OPENCLAW_CLI") {
        if std::path::Path::new(&p).exists() {
            return Ok(p);
        }
        anyhow::bail!("OPENCLAW_CLI={p} does not exist");
    }

    // 2. PATH lookup via `which`
    if let Ok(output) = Command::new("which").arg("openclaw").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }

    // 3. Common install locations
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    for candidate in [
        format!("{home}/.local/bin/openclaw"),
        format!("{home}/.cargo/bin/openclaw"),
        "/usr/local/bin/openclaw".to_string(),
    ] {
        if std::path::Path::new(&candidate).exists() {
            return Ok(candidate);
        }
    }

    anyhow::bail!("openclaw CLI not found — set OPENCLAW_CLI or install openclaw to PATH")
}

/// Spawn a fix subagent and return its session ID on success.
fn spawn_fix_subagent(label: &str, task: &str) -> Result<Option<String>> {
    log(&format!("SPAWNING FIX SUBAGENT: {label}"));

    // Write to fix queue for record
    let queue_path = fix_queue_file();
    if let Some(parent) = queue_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let entry = serde_json::json!({
        "ts": ts,
        "label": label,
        "task": task,
    });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&queue_path)?;
    serde_json::to_writer(&mut file, &entry)?;
    writeln!(file)?; // trailing newline for jsonl

    // Spawn an isolated OpenClaw agent turn in its own session.
    let session_id = format!("monitor-fix-{label}-{}", Utc::now().timestamp());
    let cli = resolve_openclaw_cli()?;
    let child = Command::new(&cli)
        .args([
            "agent",
            "--agent",
            "main",
            "--session-id",
            &session_id,
            "--message",
            task,
        ])
        .spawn();

    match child {
        Ok(_) => {
            log(&format!(
                "Spawned fix agent session: {label} ({session_id})"
            ));
            Ok(Some(session_id))
        }
        Err(e) => {
            log(&format!("Spawn failed for {label}: {e}"));
            Ok(None)
        }
    }
}

// ─── Per-repo check ───────────────────────────────────────────────────────────

fn check_repo(name: &str) -> Result<()> {
    let dir = repos_dir().join(name);

    if !dir.exists() {
        return Ok(());
    }

    log(&format!("Checking repo: {name}"));
    update_state(&format!("checking:{name}"))?;

    // 1. Push dirty repos
    push_if_dirty(&dir, name)?;

    // 2. Sync with remote
    check_remote_sync(&dir, name)?;

    // 3. Build and test (only for Rust repos with Cargo.toml)
    if dir.join("Cargo.toml").exists() {
        check_build_and_tests(&dir, name)?;
    }

    // 4. GitHub Actions CI check (requires gh CLI)
    check_ci_status(name, &dir)?;

    Ok(())
}

// ─── Escalation-driven dispatch ──────────────────────────────────────────────

/// Read pending technician escalations and dispatch fix agents for them.
///
/// This is the Phase 2.1 escalation-to-action pipeline: the monitor no longer
/// spawns fix agents on direct detection. Instead, the technician writes
/// `EscalationRecord`s to `technician-escalations.jsonl` and the monitor reads
/// them here, dispatching one fix agent per pending escalation (with budget and
/// deduplication guards).
fn process_technician_escalations() -> Result<()> {
    let esc_path = crate::technician::data::escalations_path();
    if !esc_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&esc_path).unwrap_or_default();
    if content.trim().is_empty() {
        return Ok(());
    }

    let mut records: Vec<EscalationRecord> = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<EscalationRecord>(line) {
            records.push(rec);
        }
    }

    // Track which diagnosis types already have an active dispatch (Pending or
    // Dispatched) so we don't double-spawn.
    let mut active_diagnoses: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rec in &records {
        if matches!(rec.lifecycle, EscalationLifecycle::Dispatched) {
            active_diagnoses.insert(rec.diagnosis.clone());
        }
    }

    // Budget: max 3 escalation dispatches per monitor cycle to avoid runaway
    // agent spawning.
    const MAX_DISPATCHES_PER_CYCLE: usize = 3;
    let mut dispatched_this_cycle = 0usize;
    let mut dirty = false;

    for rec in &mut records {
        if dispatched_this_cycle >= MAX_DISPATCHES_PER_CYCLE {
            break;
        }

        if rec.lifecycle != EscalationLifecycle::Pending {
            continue;
        }

        // Dedup: skip if a Dispatched record already exists for this diagnosis.
        if active_diagnoses.contains(&rec.diagnosis) {
            log(&format!(
                "escalation: skipping dup for {} (already dispatched)",
                rec.diagnosis
            ));
            continue;
        }

        // Build a contextualized task prompt from the escalation record.
        let task = format!(
            "Fix escalated issue: {diagnosis}\n\
             Escalation reason: {reason}\n\
             Context: {context}\n\
             Occurrences (24h): {count}\n\
             \n\
             Diagnose the root cause, apply the minimal fix, verify it works, \
             then commit and push.",
            diagnosis = rec.diagnosis,
            reason = rec.escalation_reason,
            context = serde_json::to_string_pretty(&rec.context).unwrap_or_default(),
            count = rec.diagnosis_count_24h,
        );

        let label = format!("esc-{}", rec.diagnosis);
        match spawn_fix_subagent(&label, &task) {
            Ok(Some(session_id)) => {
                rec.lifecycle = EscalationLifecycle::Dispatched;
                rec.dispatched_at = Some(Utc::now().to_rfc3339());
                rec.fix_agent_session_id = Some(session_id);
                active_diagnoses.insert(rec.diagnosis.clone());
                dispatched_this_cycle += 1;
                dirty = true;
                log(&format!(
                    "escalation: dispatched fix agent for {}",
                    rec.diagnosis
                ));
            }
            Ok(None) => {
                log(&format!(
                    "escalation: spawn failed for {}, will retry next cycle",
                    rec.diagnosis
                ));
            }
            Err(e) => {
                log(&format!(
                    "escalation: error dispatching {}: {e}",
                    rec.diagnosis
                ));
            }
        }
    }

    // Rewrite the escalations file with updated lifecycle states.
    if dirty {
        let mut out = String::new();
        for rec in &records {
            if let Ok(line) = serde_json::to_string(rec) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        if let Err(e) = substrate::DefaultStorage::atomic_write(esc_path.as_ref(), out.as_bytes()) {
            log(&format!(
                "escalation: failed to rewrite escalations file: {e}"
            ));
        }
    }

    if dispatched_this_cycle > 0 {
        log(&format!(
            "escalation: dispatched {dispatched_this_cycle} fix agent(s) from escalations"
        ));
    }

    Ok(())
}

// ─── Session reaping ─────────────────────────────────────────────────────────

/// Reap monitor-spawned sessions that have been running for too long.
///
/// Scans the fix-queue JSONL for sessions prefixed with "monitor-fix-" and
/// kills any that have been alive for more than 15 minutes, since fix agents
/// should complete quickly. Reaped sessions are logged and appended to the
/// fix-queue with a "reaped" status.
fn reap_stale_sessions() -> Result<()> {
    let cli = match resolve_openclaw_cli() {
        Ok(c) => c,
        Err(_) => {
            log("session reap: openclaw CLI not found, skipping");
            return Ok(());
        }
    };

    // List active sessions via openclaw
    let output = Command::new(&cli)
        .args(["sessions", "--active", "120", "--json"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            log(&format!(
                "session reap: openclaw sessions exited {}",
                o.status
            ));
            return Ok(());
        }
        Err(e) => {
            log(&format!("session reap: failed to list sessions: {e}"));
            return Ok(());
        }
    };

    let sessions: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).unwrap_or_default();

    let cutoff = Utc::now() - chrono::Duration::minutes(15);
    let mut reaped = 0usize;

    for session in &sessions {
        let id = session
            .get("session_id")
            .or_else(|| session.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !id.starts_with("monitor-fix-") {
            continue;
        }

        // Check if session is stale based on its start time
        let started = session
            .get("started_at")
            .or_else(|| session.get("created_at"))
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());

        let is_stale = match started {
            Some(ts) => ts.with_timezone(&Utc) < cutoff,
            None => false,
        };

        if !is_stale {
            continue;
        }

        log(&format!("session reap: killing stale session {id}"));
        let kill = Command::new(&cli).args(["sessions", "kill", id]).output();

        match kill {
            Ok(o) if o.status.success() => {
                reaped += 1;
            }
            Ok(o) => log(&format!("session reap: kill {id} exited {}", o.status)),
            Err(e) => log(&format!("session reap: kill {id} failed: {e}")),
        }
    }

    if reaped > 0 {
        log(&format!("session reap: reaped {reaped} stale session(s)"));
    }

    Ok(())
}

// ─── Main cycle ───────────────────────────────────────────────────────────────

fn run_monitor_cycle(repos_override: Option<&String>) -> Result<()> {
    let _lock = acquire_lock();
    log("=== Monitor cycle started ===");

    let default_repos = ["layers", "openclaw-pm", "research-radar", "council"];
    let repos: Vec<&str> = repos_override
        .as_ref()
        .map(|s| s.split(',').map(str::trim).collect())
        .unwrap_or(default_repos.to_vec());

    for name in &repos {
        if let Err(e) = check_repo(name) {
            log(&format!("Error checking {name}: {e}"));
        }
    }

    // Process technician escalations — dispatch fix agents for pending
    // escalations rather than on direct detection (Phase 2.1).
    if let Err(e) = process_technician_escalations() {
        log(&format!("Error processing escalations: {e}"));
    }

    if let Err(e) = archive_stale_council_runs() {
        log(&format!("Error archiving council runs: {e}"));
    }

    if let Err(e) = reap_stale_sessions() {
        log(&format!("Error reaping sessions: {e}"));
    }

    update_state("idle")?;
    log("=== Monitor cycle complete ===");
    Ok(())
}

// ─── Status / findings ────────────────────────────────────────────────────────

fn print_status() -> Result<()> {
    let lock_path = lock_file();
    if lock_path.exists() {
        let content = fs::read_to_string(&lock_path)?;
        println!("Lock file contents:\n{content}");
    } else {
        println!("No lock file - monitor is not running.");
    }

    if let Ok(state) = fs::read_to_string(state_file()) {
        println!("State: {}", state.trim());
    }

    if let Ok(log_text) = fs::read_to_string(log_file()) {
        let lines: Vec<_> = log_text.lines().rev().take(20).collect();
        println!("\nLast 20 log lines:");
        for line in lines.iter().rev() {
            println!("  {line}");
        }
    }

    Ok(())
}

fn print_findings() -> Result<()> {
    let path = findings_file();
    if !path.exists() {
        println!("No critical findings.");
        return Ok(());
    }
    println!("{}", fs::read_to_string(&path)?);
    Ok(())
}
