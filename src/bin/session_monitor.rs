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

/// A session returned by `openclaw sessions --active <minutes> --json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session key.
    pub key: String,
    /// Human-readable label.
    #[serde(default)]
    pub label: String,
    /// Age in milliseconds since last output (already computed by the CLI).
    #[serde(rename = "ageMs")]
    pub age_ms: u64,
    /// Session status string.
    #[serde(default)]
    pub status: String,
}

impl Session {
    /// Returns the number of seconds since this session last emitted output.
    #[must_use]
    pub fn seconds_since_update(&self) -> u64 {
        self.age_ms / 1000
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

/// Response wrapper from `openclaw sessions --active --json`.
#[derive(Deserialize)]
struct Response {
    sessions: Vec<Session>,
}

/// Call `openclaw sessions --active <minutes> --json` and parse the JSON response.
fn fetch_sessions() -> anyhow::Result<Vec<Session>> {
    let output = Command::new("openclaw")
        .args(["sessions", "--active", "120", "--json"])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "openclaw sessions failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resp: Response = serde_json::from_str(&stdout)?;
    Ok(resp.sessions)
}

// ---------------------------------------------------------------------------
// Session classification
// ---------------------------------------------------------------------------

/// Terminal / non-live statuses that should be excluded from liveness monitoring.
/// Sessions in these states are historical and should never be flagged as quiet or stalled.
const TERMINAL_STATUSES: &[&str] = &[
    "done",
    "failed",
    "lost",
    "cancelled",
    "succeeded",
    "timed_out",
];

/// Returns `true` if the session has a status that represents live/in-progress work
/// and should be subject to liveness monitoring.
#[must_use]
pub fn is_live_session(session: &Session) -> bool {
    let s = session.status.to_lowercase();
    !TERMINAL_STATUSES.contains(&s.as_str())
}

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
/// Sessions with terminal statuses are excluded entirely — they are not live work.
#[must_use]
pub fn partition_sessions(
    thresholds: &Thresholds,
    sessions: &[Session],
) -> (Vec<Session>, Vec<Session>, Vec<Session>) {
    let mut ok = Vec::new();
    let mut quiet = Vec::new();
    let mut stalled = Vec::new();

    for session in sessions {
        if !is_live_session(session) {
            continue;
        }
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
    format!(
        "  - [{}] {} (key={}, status={}, age={})",
        session.label, age, session.key, session.status, age
    )
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
        writeln!(
            file,
            "- **{}** (key={}, status={}, age={}s)",
            session.label, session.key, session.status, secs
        )?;
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
        eprintln!(
            "session-monitor: could not acquire lock ({e}) — another instance is likely running. Exiting."
        );
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

    // 4. Partition by state (non-live sessions are excluded).
    let (ok, quiet, stalled) = partition_sessions(&thresholds, &sessions);
    let live_count = ok.len() + quiet.len() + stalled.len();
    let skipped = sessions.len() - live_count;

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
            "session-monitor: {} quiet, {} stalled out of {} live sessions ({} non-live skipped)",
            quiet.len(),
            stalled.len(),
            live_count,
            skipped
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
        // Session updated 30 seconds ago.
        let s = Session {
            key: "test".into(),
            label: "test-label".into(),
            age_ms: 30_000,
            status: "running".into(),
        };

        let delta = s.seconds_since_update();
        assert!(delta >= 29 && delta <= 31, "expected ~30s, got {delta}");
    }

    // ── classify ────────────────────────────────────────────────────────────

    #[test]
    fn classify_ok_when_within_quiet_threshold() {
        let t = Thresholds {
            quiet_secs: 180,
            stalled_secs: 420,
        };

        let s = Session {
            key: "k".into(),
            label: "l".into(),
            age_ms: 60_000,
            status: "running".into(),
        };
        assert_eq!(classify(&t, &s), SessionState::Ok);
    }

    #[test]
    fn classify_quiet_when_between_thresholds() {
        let t = Thresholds {
            quiet_secs: 180,
            stalled_secs: 420,
        };

        let s = Session {
            key: "k".into(),
            label: "l".into(),
            age_ms: 200_000,
            status: "running".into(),
        };
        assert_eq!(classify(&t, &s), SessionState::Quiet { secs: 200 });
    }

    #[test]
    fn classify_stalled_when_past_stalled_threshold() {
        let t = Thresholds {
            quiet_secs: 180,
            stalled_secs: 420,
        };

        let s = Session {
            key: "k".into(),
            label: "l".into(),
            age_ms: 500_000,
            status: "running".into(),
        };
        assert_eq!(classify(&t, &s), SessionState::Stalled { secs: 500 });
    }

    // ── partition_sessions ─────────────────────────────────────────────────

    #[test]
    fn partition_empty() {
        let t = Thresholds {
            quiet_secs: 180,
            stalled_secs: 420,
        };
        let (ok, quiet, stalled) = partition_sessions(&t, &[]);
        assert!(ok.is_empty() && quiet.is_empty() && stalled.is_empty());
    }

    #[test]
    fn partition_mixed() {
        let t = Thresholds {
            quiet_secs: 180,
            stalled_secs: 420,
        };

        let sessions = vec![
            Session {
                key: "ok".into(),
                label: "ok-label".into(),
                age_ms: 30_000,
                status: "running".into(),
            },
            Session {
                key: "quiet".into(),
                label: "quiet-label".into(),
                age_ms: 200_000,
                status: "running".into(),
            },
            Session {
                key: "stalled".into(),
                label: "stalled-label".into(),
                age_ms: 500_000,
                status: "running".into(),
            },
            Session {
                key: "quiet2".into(),
                label: "quiet2-label".into(),
                age_ms: 300_000,
                status: "running".into(),
            },
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

    // ── is_live_session / status filtering ─────────────────────────────────

    #[test]
    fn live_session_running() {
        let s = Session {
            key: "k".into(),
            label: "l".into(),
            age_ms: 0,
            status: "running".into(),
        };
        assert!(is_live_session(&s));
    }

    #[test]
    fn live_session_empty_status() {
        let s = Session {
            key: "k".into(),
            label: "l".into(),
            age_ms: 0,
            status: String::new(),
        };
        assert!(is_live_session(&s), "empty status should be treated as live");
    }

    #[test]
    fn non_live_terminal_statuses() {
        for status in &["done", "failed", "lost", "cancelled", "succeeded", "timed_out"] {
            let s = Session {
                key: "k".into(),
                label: "l".into(),
                age_ms: 999_999,
                status: (*status).to_string(),
            };
            assert!(
                !is_live_session(&s),
                "status '{status}' should NOT be live"
            );
        }
    }

    #[test]
    fn non_live_case_insensitive() {
        let s = Session {
            key: "k".into(),
            label: "l".into(),
            age_ms: 0,
            status: "Failed".into(),
        };
        assert!(!is_live_session(&s), "case-insensitive match should work");
    }

    #[test]
    fn partition_skips_terminal_sessions() {
        let t = Thresholds {
            quiet_secs: 180,
            stalled_secs: 420,
        };

        let sessions = vec![
            Session {
                key: "active".into(),
                label: "active-label".into(),
                age_ms: 30_000,
                status: "running".into(),
            },
            Session {
                key: "done-old".into(),
                label: "done-label".into(),
                age_ms: 999_000,
                status: "done".into(),
            },
            Session {
                key: "failed-old".into(),
                label: "failed-label".into(),
                age_ms: 800_000,
                status: "failed".into(),
            },
            Session {
                key: "stalled-live".into(),
                label: "stalled-label".into(),
                age_ms: 500_000,
                status: "running".into(),
            },
        ];

        let (ok, quiet, stalled) = partition_sessions(&t, &sessions);
        assert_eq!(ok.len(), 1, "only the active running session");
        assert_eq!(ok[0].key, "active");
        assert!(quiet.is_empty(), "no quiet sessions");
        assert_eq!(stalled.len(), 1, "only the live stalled session");
        assert_eq!(stalled[0].key, "stalled-live");
    }

    // ── JSON parsing ─────────────────────────────────────────────────────────

    #[test]
    fn parse_sessions_json() {
        let json = r#"[
          {"key": "abc123", "label": "subagent-1", "ageMs": 60000, "status": "running"},
          {"key": "def456", "label": "subagent-2", "ageMs": 120000, "status": "idle"}
        ]"#;

        let sessions: Vec<Session> = serde_json::from_str(json).expect("should parse");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].key, "abc123");
        assert_eq!(sessions[0].label, "subagent-1");
        assert_eq!(sessions[0].age_ms, 60000);
        assert_eq!(sessions[0].seconds_since_update(), 60);
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
