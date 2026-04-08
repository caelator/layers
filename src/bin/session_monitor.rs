//! Session liveness monitor binary.
//!
//! Runs on a cron schedule, checks all active `OpenClaw` sessions for liveness,
//! and flags quietly stalled or dead sessions before they waste time.
//!
//! Exit codes:
//! - 0: always exits 0 (findings are written to files)
//! - 1: only on unexpected error (lock acquisition failed, parse error, etc.)

#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const LAYERS_DIR: &str = ".layers";
const LOCK_FILE: &str = ".session-monitor.lock";
const LOG_FILE: &str = ".session-monitor.log";
const CRITICAL_FILE: &str = ".critical-findings.md";

/// Default quiet threshold in seconds (3 minutes).
const DEFAULT_QUIET_THRESHOLD_SECS: u64 = 180;
/// Default stalled threshold in seconds (7 minutes).
const DEFAULT_STALLED_THRESHOLD_SECS: u64 = 420;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// A session returned by `openclaw sessions list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session key.
    pub key: String,
    /// Human-readable label.
    pub label: String,
    /// Unix timestamp (milliseconds) of last activity.
    #[serde(rename = "updatedAt")]
    pub updated_at: u64,
    /// Session status string.
    pub status: String,
}

impl Session {
    /// Returns the number of seconds since this session last emitted output.
    #[must_use]
    pub fn seconds_since_update(&self) -> u64 {
        #[allow(clippy::cast_possible_truncation, clippy::missing_panics_doc)]
        let now_ms = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before epoch")
                .as_millis(),
        )
        .expect("milliseconds since epoch exceeds u64::MAX");
        now_ms.saturating_sub(self.updated_at) / 1000
    }
}

/// Categorisation of a session based on its last-update age.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Session is actively producing output.
    Ok,
    /// Session has been quiet for longer than `quiet_threshold` but less than `stalled_threshold`.
    Quiet { secs: u64 },
    /// Session has been stalled for longer than `stalled_threshold`.
    Stalled { secs: u64 },
}

/// Thresholds used to classify session liveness.
#[derive(Debug, Clone)]
pub struct Thresholds {
    pub quiet_secs: u64,
    pub stalled_secs: u64,
}

impl Thresholds {
    /// Construct from environment variables or fall back to defaults.
    fn from_env() -> Self {
        Self {
            quiet_secs: std::env::var("QUIET_THRESHOLD_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_QUIET_THRESHOLD_SECS),
            stalled_secs: std::env::var("STALLED_THRESHOLD_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_STALLED_THRESHOLD_SECS),
        }
    }
}

// ---------------------------------------------------------------------------
// Lock file
// ---------------------------------------------------------------------------

/// Path to the layers directory (~/.layers).
fn layers_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(LAYERS_DIR)
}

/// Path to the lock file.
fn lock_path() -> PathBuf {
    layers_dir().join(LOCK_FILE)
}

/// Path to the log file for quiet sessions.
fn log_path() -> PathBuf {
    layers_dir().join(LOG_FILE)
}

/// Path to the critical findings file.
fn critical_path() -> PathBuf {
    layers_dir().join(CRITICAL_FILE)
}

/// Acquire an exclusive lock by writing our PID to the lock file.
/// Returns an error if another instance is already holding the lock.
#[allow(unsafe_code)]
fn acquire_lock() -> io::Result<()> {
    let dir = layers_dir();
    fs::create_dir_all(&dir)?;

    let lock_file = lock_path();

    // Check for stale lock: if file exists but PID is dead, we can take over.
    if let Ok(contents) = fs::read_to_string(&lock_file) {
        if let Some(pid_str) = contents.strip_prefix("pid: ") {
            let pid_str = pid_str.trim();
            if let Ok(pid) = pid_str.parse::<u32>() {
                // On Unix, kill(pid, 0) checks existence without sending a signal.
                #[cfg(unix)]
                {
                    #[allow(clippy::cast_possible_wrap)]
                    let pid_t: libc::pid_t = pid as libc::pid_t;
                    if unsafe { libc::kill(pid_t, 0) } == 0 {
                        // PID is alive — lock is held.
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            format!("lock held by PID {pid}"),
                        ));
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = pid;
                    // On non-Unix just proceed.
                }
            }
        }
    }

    let pid = std::process::id();
    let timestamp = Utc::now().to_rfc3339();
    fs::write(&lock_file, format!("pid: {pid}\nstarted: {timestamp}\n"))?;
    Ok(())
}

/// Release the lock file.
fn release_lock() {
    let _ = fs::remove_file(lock_path());
}

// ---------------------------------------------------------------------------
// OpenClaw sessions API
// ---------------------------------------------------------------------------

/// Call `openclaw sessions list` and parse the JSON response.
fn fetch_sessions() -> anyhow::Result<Vec<Session>> {
    let output = Command::new("openclaw")
        .args(["sessions", "list"])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "openclaw sessions list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let sessions: Vec<Session> = serde_json::from_str(&stdout)?;
    Ok(sessions)
}

// ---------------------------------------------------------------------------
// Session classification
// ---------------------------------------------------------------------------

/// Classify a session's state given the configured thresholds.
fn classify(thresholds: &Thresholds, session: &Session) -> SessionState {
    let secs = session.seconds_since_update();
    if secs >= thresholds.stalled_secs {
        SessionState::Stalled { secs }
    } else if secs >= thresholds.quiet_secs {
        SessionState::Quiet { secs }
    } else {
        SessionState::Ok
    }
}

/// Partition sessions into ok, quiet, and stalled buckets.
#[must_use]
pub fn partition_sessions(
    thresholds: &Thresholds,
    sessions: &[Session],
) -> (Vec<Session>, Vec<Session>, Vec<Session>) {
    let mut ok = Vec::new();
    let mut quiet = Vec::new();
    let mut stalled = Vec::new();

    for session in sessions {
        match classify(thresholds, session) {
            SessionState::Ok => ok.push(session.clone()),
            SessionState::Quiet { .. } => quiet.push(session.clone()),
            SessionState::Stalled { .. } => stalled.push(session.clone()),
        }
    }

    (ok, quiet, stalled)
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Format a session for human-readable output.
fn format_session(session: &Session, secs: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let age = if secs >= 60 {
        format!("{:.1}m", secs as f64 / 60.0)
    } else {
        format!("{secs}s")
    };
    format!("  - [{}] {} (key={}, status={}, age={})", session.label, age, session.key, session.status, age)
}

/// Write quiet session report to the log file.
fn write_quiet_log(quiet_sessions: &[Session]) -> io::Result<()> {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())?;

    let ts = Utc::now().to_rfc3339();
    writeln!(file, "\n--- quiet check: {ts} ---")?;
    for session in quiet_sessions {
        let secs = session.seconds_since_update();
        writeln!(file, "{}", format_session(session, secs))?;
    }

    Ok(())
}

/// Write stalled session findings to the critical-findings file.
/// Uses the same markdown format as autonomous-monitor.
fn write_stalled_critical(stalled_sessions: &[Session]) -> io::Result<()> {
    use std::io::Write;

    if stalled_sessions.is_empty() {
        return Ok(());
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(critical_path())?;

    let ts = Utc::now().to_rfc3339();
    writeln!(file)?;
    writeln!(file, "## [{ts}] critical | session-monitor")?;
    writeln!(file)?;
    writeln!(file, "Stalled subagent sessions detected:")?;
    writeln!(file)?;
    for session in stalled_sessions {
        let secs = session.seconds_since_update();
        writeln!(file, "- **{}** (key={}, status={}, age={}s)", session.label, session.key, session.status, secs)?;
    }
    writeln!(file)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    if let Err(e) = run() {
        eprintln!("session-monitor error: {e}");
        std::process::exit(1);
    }
}

/// Returns `Ok(())` on clean exit (including "another instance running").
/// Returns `Err` only on truly unexpected internal errors.
#[allow(clippy::unnecessary_wraps)]
fn run() -> anyhow::Result<()> {
    // 1. Acquire lock — exit cleanly if another instance is running.
    if let Err(e) = acquire_lock() {
        eprintln!("session-monitor: could not acquire lock ({e}) — another instance is likely running. Exiting.");
        return Ok(()); // Not an error — just exit.
    }

    // On panic the hook fires; on normal exit DropGuard releases the lock.
    std::panic::set_hook(Box::new(|_| release_lock()));
    let _guard = DropGuard;

    // 2. Load thresholds.
    let thresholds = Thresholds::from_env();

    // 3. Fetch active sessions.
    let sessions = match fetch_sessions() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("session-monitor: failed to fetch sessions: {e}");
            return Ok(()); // Exit cleanly — don't write to critical.
        }
    };

    // 4. Partition by state.
    let (_ok, quiet, stalled) = partition_sessions(&thresholds, &sessions);

    // 5. Output per state.
    if !quiet.is_empty() {
        if let Err(e) = write_quiet_log(&quiet) {
            eprintln!("session-monitor: failed to write quiet log: {e}");
        }
    }

    if !stalled.is_empty() {
        if let Err(e) = write_stalled_critical(&stalled) {
            eprintln!("session-monitor: failed to write critical findings: {e}");
        }
    }

    // 6. Log summary on any finding.
    if !quiet.is_empty() || !stalled.is_empty() {
        eprintln!(
            "session-monitor: {} quiet, {} stalled out of {} total sessions",
            quiet.len(),
            stalled.len(),
            sessions.len()
        );
    }

    Ok(())
}

/// RAII guard that releases the lock on drop.
struct DropGuard;

impl Drop for DropGuard {
    fn drop(&mut self) {
        release_lock();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Session::seconds_since_update ─────────────────────────────────────

    #[test]
    fn seconds_since_update_computes_correct_delta() {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_millis() as u64;

        // Session updated 30 seconds ago.
        let s = Session {
            key: "test".into(),
            label: "test-label".into(),
            updated_at: now_ms - 30_000,
            status: "running".into(),
        };

        let delta = s.seconds_since_update();
        assert!(delta >= 29 && delta <= 31, "expected ~30s, got {delta}");
    }

    // ── classify ────────────────────────────────────────────────────────────

    #[test]
    fn classify_ok_when_within_quiet_threshold() {
        let t = Thresholds { quiet_secs: 180, stalled_secs: 420 };
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_millis() as u64;

        let s = Session { key: "k".into(), label: "l".into(), updated_at: now_ms - 60_000, status: "running".into() };
        assert_eq!(classify(&t, &s), SessionState::Ok);
    }

    #[test]
    fn classify_quiet_when_between_thresholds() {
        let t = Thresholds { quiet_secs: 180, stalled_secs: 420 };
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_millis() as u64;

        let s = Session { key: "k".into(), label: "l".into(), updated_at: now_ms - 200_000, status: "running".into() };
        assert_eq!(classify(&t, &s), SessionState::Quiet { secs: 200 });
    }

    #[test]
    fn classify_stalled_when_past_stalled_threshold() {
        let t = Thresholds { quiet_secs: 180, stalled_secs: 420 };
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_millis() as u64;

        let s = Session { key: "k".into(), label: "l".into(), updated_at: now_ms - 500_000, status: "running".into() };
        assert_eq!(classify(&t, &s), SessionState::Stalled { secs: 500 });
    }

    // ── partition_sessions ─────────────────────────────────────────────────

    #[test]
    fn partition_empty() {
        let t = Thresholds { quiet_secs: 180, stalled_secs: 420 };
        let (ok, quiet, stalled) = partition_sessions(&t, &[]);
        assert!(ok.is_empty() && quiet.is_empty() && stalled.is_empty());
    }

    #[test]
    fn partition_mixed() {
        let t = Thresholds { quiet_secs: 180, stalled_secs: 420 };
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_millis() as u64;

        let sessions = vec![
            Session { key: "ok".into(),     label: "ok-label".into(),     updated_at: now_ms - 30_000,  status: "running".into() },
            Session { key: "quiet".into(),  label: "quiet-label".into(),  updated_at: now_ms - 200_000, status: "running".into() },
            Session { key: "stalled".into(),label: "stalled-label".into(),updated_at: now_ms - 500_000, status: "running".into() },
            Session { key: "quiet2".into(), label: "quiet2-label".into(),updated_at: now_ms - 300_000, status: "running".into() },
        ];

        let (ok, quiet, stalled) = partition_sessions(&t, &sessions);
        assert_eq!(ok.len(), 1);
        assert_eq!(quiet.len(), 2);
        assert_eq!(stalled.len(), 1);
        assert_eq!(ok[0].key, "ok");
        assert_eq!(quiet[0].key, "quiet");
        assert_eq!(stalled[0].key, "stalled");
    }

    // ── Thresholds from env ─────────────────────────────────────────────────

    #[allow(unsafe_code)]
    #[test]
    fn thresholds_default_when_env_unset() {
        // Unset the vars if they are set.
        unsafe {
            std::env::remove_var("QUIET_THRESHOLD_SECS");
            std::env::remove_var("STALLED_THRESHOLD_SECS");
        }
        let t = Thresholds::from_env();
        assert_eq!(t.quiet_secs, 180);
        assert_eq!(t.stalled_secs, 420);
    }

    #[allow(unsafe_code)]
    #[test]
    fn thresholds_from_env_vars() {
        unsafe {
            std::env::set_var("QUIET_THRESHOLD_SECS", "60");
            std::env::set_var("STALLED_THRESHOLD_SECS", "300");
        }
        let t = Thresholds::from_env();
        assert_eq!(t.quiet_secs, 60);
        assert_eq!(t.stalled_secs, 300);
        unsafe {
            std::env::remove_var("QUIET_THRESHOLD_SECS");
            std::env::remove_var("STALLED_THRESHOLD_SECS");
        }
    }

    // ── JSON parsing ─────────────────────────────────────────────────────────

    #[test]
    fn parse_sessions_json() {
        let json = r#"[
          {"key": "abc123", "label": "subagent-1", "updatedAt": 1744128000000, "status": "running"},
          {"key": "def456", "label": "subagent-2", "updatedAt": 1744128100000, "status": "idle"}
        ]"#;

        let sessions: Vec<Session> = serde_json::from_str(json).expect("should parse");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].key, "abc123");
        assert_eq!(sessions[0].label, "subagent-1");
        assert_eq!(sessions[0].updated_at, 1_744_128_000_000);
        assert_eq!(sessions[0].status, "running");
    }
}

// ---------------------------------------------------------------------------
// Build compatibility shims
// ---------------------------------------------------------------------------

mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
    }
}
