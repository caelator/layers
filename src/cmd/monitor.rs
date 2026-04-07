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
            log(&format!("Rebase conflict: {name} - spawning fix subagent"));
            record_finding(
                Severity::Critical,
                name,
                &format!("Rebase conflict in {}", dir.display()),
            );
            spawn_fix_subagent(
                &format!("rebase-{name}"),
                &format!(
                    "Fix rebase conflict in {name}:\ngit status to see the conflict files,\nresolve with git add/rm, then git rebase --continue"
                ),
            )?;
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
        spawn_fix_subagent(
            &format!("build-{name}"),
            &format!(
                "Fix build errors in {}:\n\
                 cargo build --release 2>&1 | grep error to see errors\n\
                 Apply minimal fixes (typos, missing imports, API changes)\n\
                 Run cargo build --release && cargo test\n\
                 Commit and push if all pass",
                dir.display()
            ),
        )?;
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
            spawn_fix_subagent(
                &format!("tests-{name}"),
                &format!(
                    "Fix failing tests in {dir}:\n\
                     cargo test 2>&1 to see all failures\n\
                     Read failing test code and fix the underlying code\n\
                     Do NOT change tests unless the test itself is wrong\n\
                     Run cargo test to verify all pass\n\
                     Commit and push if all pass",
                    dir = dir.display()
                ),
            )?;
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
            spawn_fix_subagent(
                &format!("tests-{name}"),
                &format!(
                    "Fix test compilation errors in {dir}:\n\
                     cargo build --tests 2>&1 to see all errors\n\
                     Apply minimal fixes\n\
                     Run cargo test to verify all pass\n\
                     Commit and push if all pass",
                    dir = dir.display()
                ),
            )?;
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
        spawn_fix_subagent(
            &format!("ci-{name}"),
            &format!(
                "Fix GitHub Actions CI failure in caelator/{name}:\n\
                 gh run list --repo caelator/{name} --limit 3 to see recent runs\n\
                 gh run view <run-id> --log-failed to get failure logs\n\
                 Reproduce locally: cd {} && cargo test\n\
                 Apply minimal fix to make CI green\n\
                 Push to trigger CI again\n\
                 Verify: gh run list --repo caelator/{name} --limit 3",
                dir.display()
            ),
        )?;
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

fn spawn_fix_subagent(label: &str, task: &str) -> Result<()> {
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
    // Local CLI help confirms `openclaw agent` is the supported non-interactive
    // entrypoint, while `openclaw sessions` only supports listing/cleanup.
    let session_id = format!("monitor-fix-{label}-{}", Utc::now().timestamp());
    let child = Command::new(std::env::var("OPENCLAW_CLI").unwrap_or_else(|_| "openclaw".into()))
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
        Ok(_) => log(&format!(
            "Spawned fix agent session: {label} ({session_id})"
        )),
        Err(e) => log(&format!("Spawn failed for {label}: {e}")),
    }

    Ok(())
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

    if let Err(e) = archive_stale_council_runs() {
        log(&format!("Error archiving council runs: {e}"));
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
