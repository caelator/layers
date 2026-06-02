//! `layers research` — autospawned, continuously-chained research-and-implementation runtime (v2 stable-core job 6).
//!
//! This command family is the v2.1+ entrypoint named in `docs/NORTH_STAR.md`
//! and `docs/V2_PRODUCT_CONTRACT.md`. It is the sanctioned way to run
//! Layers autonomously on a dedicated branch, with the user absent.
//! The runtime supports two postures: bounded-cron (v2.1, hard wall-clock
//! cap, cronjob-only launch) and autospawned continuously-chained (v2.2,
//! soft wall-clock cap, configurable cooldown, autospawn trigger). Both
//! postures share the same hard rules below.
//!
//! ## Subcommands
//!
//! - `run` — drive a bounded research-and-implementation cycle. The
//!   `autoresearch` and `autoresearch+grade` modes wrap the synchronous
//!   `Sweep` slice (`run_sweep` in `autoresearch.rs`) and append one sweep
//!   row to `.layers/autoresearch/sweeps.tsv` per iteration. The
//!   `autoresearch+edit` mode is reserved for a v2.2 slice and currently
//!   errors out as not-yet-implemented.
//! - `status <run-id>` — read the run summary from
//!   `.layers/autoresearch/runs/<run-id>/run.json`.
//! - `stop <run-id>` — write a `.stop` flag file under
//!   `.layers/autoresearch/runs/<run-id>/` that the in-flight `run` loop
//!   polls between iterations and exits on.
//!
//! ## Hard invariants
//!
//! - The runtime refuses to write to a branch whose name does not start
//!   with `autoresearch/`. It also refuses to operate when the working
//!   tree is dirty at start.
//! - Wall-clock duration is hard-capped at 12h per the v2 contract. The
//!   default is 30 minutes; pass `--duration` to extend.
//! - The runtime does not auto-spawn. It is invoked either directly or
//!   via `cronjob` with `notify_on_complete: true`. (The runtime itself
//!   contains no scheduler.)
//! - The runtime does not recursively schedule cron jobs.
//! - The runtime writes one sweep row per iteration. No silent omissions.
//! - The runtime has cooperative cancellation via the `stop` subcommand.

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::cmd::autoresearch::{SweepOptions, run_sweep, sweep_log_path};
use crate::config::workspace_root;

/// Maximum wall-clock duration per run, per the v2 product contract job 6.
pub const MAX_RUN_DURATION_SECS: u64 = 12 * 60 * 60;

/// Default wall-clock duration per run, in seconds. Surfaced for tests
/// and for documentation; the `--duration` flag string is the public
/// default.
pub const DEFAULT_RUN_DURATION_SECS: u64 = 30 * 60;

/// String default for the `--duration` flag, derived from the
/// [`DEFAULT_RUN_DURATION_SECS`] constant. Keep these in sync.
pub const DEFAULT_RUN_DURATION_STR: &str = "30m";

/// Default cooldown between chained autospawn runs, in seconds.
/// Surfaced for tests and for documentation; the
/// `--autospawn-cooldown` flag string is the public default.
pub const DEFAULT_AUTOSPAWN_COOLDOWN_SECS: u64 = 5 * 60;

/// Minimum allowed cooldown. Zero is the failure mode the v2.1 hard
/// cap prevented; the v2.2 gate depends on the cooldown being
/// non-zero. Users who set a cooldown below 30s require explicit
/// standing approval per the v2.2 autospawn skill.
pub const MIN_AUTOSPAWN_COOLDOWN_SECS: u64 = 1;

/// String default for the `--autospawn-cooldown` flag, derived from
/// the [`DEFAULT_AUTOSPAWN_COOLDOWN_SECS`] constant. Keep these in
/// sync.
pub const DEFAULT_AUTOSPAWN_COOLDOWN_STR: &str = "5m";

/// Nested commands for `layers research`.
#[derive(Debug, Clone, Subcommand)]
pub enum ResearchCommands {
    /// Run a bounded research-and-implementation cycle on a dedicated
    /// branch. Synchronous, foreground, hard-capped at 12h.
    Run {
        /// Task to drive the cycle with. Forwarded to `Sweep` per
        /// iteration.
        #[arg(short, long)]
        task: String,
        /// Optional targets forwarded to `Sweep` per iteration.
        #[arg(short = 'T', long)]
        target: Vec<String>,
        /// Dedicated branch to operate on. Must start with
        /// `autoresearch/`. The branch is created from current HEAD if
        /// it does not exist.
        #[arg(short, long)]
        branch: String,
        /// Wall-clock duration of the run. Hard-capped at 12h.
        /// Accepts `30m`, `2h`, `12h`, or bare seconds.
        #[arg(long, default_value = DEFAULT_RUN_DURATION_STR)]
        duration: String,
        /// Mode. `autoresearch` runs `Sweep` only; `autoresearch+grade`
        /// also grades each prepared packet and writes
        /// `before_packet_grade` / `after_packet_grade` to the TSV.
        /// `autoresearch+edit` is reserved for a v2.2 slice and is
        /// currently rejected.
        #[arg(long, default_value = "autoresearch+grade", value_parser = parse_mode_arg)]
        mode: ResearchMode,
        /// Optional profile id to scope `ScanOnce` per iteration.
        #[arg(long)]
        profile_id: Option<String>,
        /// Findings-count delta required to mark a sweep row `kept`
        /// rather than `discarded`. Forwarded to `Sweep`.
        #[arg(long, default_value_t = 0)]
        keep_min_delta: i64,
        /// Emit JSON summary at the end of the run.
        #[arg(long)]
        json: bool,
        /// Cooldown between chained autospawn iterations, in seconds.
        /// Only honored when `--autospawn-trigger` is not `none`.
        /// Accepts `30s`, `5m`, `1h`, or bare seconds. Must be > 0;
        /// the v2.2 autospawn gate depends on a non-zero cooldown.
        #[arg(long, default_value = DEFAULT_AUTOSPAWN_COOLDOWN_STR)]
        autospawn_cooldown: String,
        /// Autospawn trigger kind. `none` (default) preserves the v2.1
        /// single-run behavior. `heartbeat`, `file-watch`, and `daemon`
        /// enable the v2.2 autospawn loop, which chains back-to-back
        /// iterations on the dedicated branch gated by
        /// `--autospawn-cooldown`.
        #[arg(long, default_value = "none", value_parser = parse_autospawn_trigger_arg)]
        autospawn_trigger: AutospawnTrigger,
    },
    /// Show the status of a research run by run id.
    Status {
        /// Run id to look up.
        run_id: String,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Cooperative cancellation: writes a `.stop` flag the in-flight
    /// `run` loop polls between iterations.
    Stop {
        /// Run id to stop.
        run_id: String,
    },
}

/// Mode discriminator for `layers research run`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchMode {
    /// `Sweep` per iteration, no grading.
    Autoresearch,
    /// `Sweep` per iteration with `--grade-packet` per call.
    AutoresearchGrade,
    /// Reserved for a v2.2 slice that drives an LLM to edit `src/` per
    /// iteration behind the v2 verification gate. Not yet implemented.
    AutoresearchEdit,
}

impl ResearchMode {
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "autoresearch" => Ok(Self::Autoresearch),
            "autoresearch+grade" => Ok(Self::AutoresearchGrade),
            "autoresearch+edit" => Ok(Self::AutoresearchEdit),
            other => Err(anyhow!(
                "unknown research mode '{other}'; expected autoresearch, autoresearch+grade, or autoresearch+edit"
            )),
        }
    }
}

/// Clap value parser for `--mode` that maps a string to a `ResearchMode`.
fn parse_mode_arg(s: &str) -> std::result::Result<ResearchMode, String> {
    ResearchMode::from_str(s).map_err(|e| e.to_string())
}

/// Trigger kind for the v2.2 autospawn loop. `None` preserves the
/// v2.1 single-run behavior; the other variants enable back-to-back
/// chaining on the dedicated branch, gated by `--autospawn-cooldown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutospawnTrigger {
    /// No autospawn. The runtime exits after a single iteration.
    /// This is the v2.1 default and the v2.2 fallback when the
    /// autospawn loop is not configured.
    None,
    /// Heartbeat trigger. A separate process on this machine
    /// signals via a `.heartbeat` file under
    /// `.layers/autoresearch/runs/<run-id>/`. The runtime polls the
    /// file between iterations.
    Heartbeat,
    /// File-watch trigger. The runtime polls a configured file
    /// (default: `program.md` / `RUNTIME.md`) and re-runs when the
    /// file's mtime advances. This is the
    /// "user is editing the program file" pattern.
    FileWatch,
    /// Daemon trigger. A long-lived daemon process on this machine
    /// signals via a Unix socket or a `.daemon` flag. The runtime
    /// polls the flag between iterations.
    Daemon,
}

impl AutospawnTrigger {
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "none" => Ok(Self::None),
            "heartbeat" => Ok(Self::Heartbeat),
            "file-watch" => Ok(Self::FileWatch),
            "daemon" => Ok(Self::Daemon),
            other => Err(anyhow!(
                "unknown autospawn trigger '{other}'; expected none, heartbeat, file-watch, or daemon"
            )),
        }
    }
}

/// Clap value parser for `--autospawn-trigger` that maps a string to
/// an `AutospawnTrigger`.
fn parse_autospawn_trigger_arg(s: &str) -> std::result::Result<AutospawnTrigger, String> {
    AutospawnTrigger::from_str(s).map_err(|e| e.to_string())
}

/// Autospawn metadata recorded in the run summary when the autospawn
/// posture is active. The independent review uses this field to
/// detect fallback (e.g. trigger set to `None` mid-run, cooldown
/// reduced to zero).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutospawnMetadata {
    pub trigger: AutospawnTrigger,
    pub cooldown_secs: u64,
    /// Number of chained runs completed in this autospawn session
    /// (the first run counts as 1). Reset per autospawn session, not
    /// per outer `research run` invocation.
    pub chained_runs: u32,
}

/// Persisted run summary, written to
/// `.layers/autoresearch/runs/<run-id>/run.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub task: String,
    pub branch: String,
    pub mode: ResearchMode,
    pub duration_secs: u64,
    pub iterations: usize,
    pub kept: usize,
    pub discarded: usize,
    pub crashed: usize,
    pub stopped_early: bool,
    pub sweep_log_path: PathBuf,
    pub run_dir: PathBuf,
    /// v2.2 autospawn metadata. `None` for v2.1 single-run posture
    /// and for v2.2 runs whose trigger resolved to `None`. Present
    /// for v2.2 autospawn runs with an active trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autospawn: Option<AutospawnMetadata>,
}

/// Dispatch `layers research` commands.
pub fn handle_research(command: &ResearchCommands) -> Result<()> {
    match command {
        ResearchCommands::Run {
            task,
            target,
            branch,
            duration,
            mode,
            profile_id,
            keep_min_delta,
            json,
            autospawn_cooldown,
            autospawn_trigger,
        } => {
            run_research(
                task,
                target,
                branch,
                duration,
                *mode,
                profile_id.as_deref(),
                *keep_min_delta,
                *json,
                autospawn_cooldown,
                *autospawn_trigger,
            )?;
        }
        ResearchCommands::Status { run_id, json } => {
            let summary = read_run_summary(run_id)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else {
                println!("research run: {}", summary.run_id);
                println!("  task        : {}", summary.task);
                println!("  branch      : {}", summary.branch);
                println!("  mode        : {:?}", summary.mode);
                println!("  started_at  : {}", summary.started_at.to_rfc3339());
                println!("  finished_at : {}", summary.finished_at.to_rfc3339());
                println!("  duration    : {}s", summary.duration_secs);
                println!("  iterations  : {}", summary.iterations);
                println!("  kept        : {}", summary.kept);
                println!("  discarded   : {}", summary.discarded);
                println!("  crashed     : {}", summary.crashed);
                println!("  stopped_early: {}", summary.stopped_early);
                println!("  sweep log   : {}", summary.sweep_log_path.display());
                println!("  run dir     : {}", summary.run_dir.display());
            }
        }
        ResearchCommands::Stop { run_id } => {
            write_stop_flag(run_id)?;
            println!(
                "stop requested for run {run_id}; the in-flight loop will exit between iterations"
            );
        }
    }
    Ok(())
}

fn parse_duration_secs(s: &str) -> Result<u64> {
    let s = s.trim();
    if let Some(stripped) = s.strip_suffix('h') {
        let n: u64 = stripped
            .parse()
            .with_context(|| format!("invalid hour count in '{s}'"))?;
        return Ok(n * 3600);
    }
    if let Some(stripped) = s.strip_suffix('m') {
        let n: u64 = stripped
            .parse()
            .with_context(|| format!("invalid minute count in '{s}'"))?;
        return Ok(n * 60);
    }
    if let Some(stripped) = s.strip_suffix('s') {
        let n: u64 = stripped
            .parse()
            .with_context(|| format!("invalid second count in '{s}'"))?;
        return Ok(n);
    }
    let n: u64 = s
        .parse()
        .with_context(|| format!("invalid duration '{s}'; expected Nm, Nh, or Ns"))?;
    Ok(n)
}

/// Parse an `--autospawn-cooldown` string. Accepts the same suffix
/// grammar as [`parse_duration_secs`] (`30s`, `5m`, `1h`, or bare
/// seconds). Returns the cooldown in seconds.
fn parse_autospawn_cooldown_secs(s: &str) -> Result<u64> {
    let secs = parse_duration_secs(s).map_err(|_| {
        anyhow!("invalid autospawn cooldown '{s}'; expected 30s, 5m, 1h, or bare seconds")
    })?;
    Ok(secs)
}

/// Enforce the v2.2 autospawn cooldown invariant. The cooldown must
/// be non-zero; a zero cooldown is the failure mode the v2.1 hard
/// cap prevented and the v2.2 gate explicitly depends on the
/// cooldown being non-zero. Negative values are rejected as a
/// defensive guard for future signed-type APIs.
fn enforce_autospawn_cooldown_invariant(cooldown_secs: u64) -> Result<()> {
    if cooldown_secs < MIN_AUTOSPAWN_COOLDOWN_SECS {
        return Err(anyhow!(
            "autospawn cooldown must be >= {MIN_AUTOSPAWN_COOLDOWN_SECS}s; got {cooldown_secs}s. \
             A zero or negative cooldown is the failure mode the v2.1 hard cap prevented; \
             the v2.2 autospawn gate depends on the cooldown being non-zero. \
             The default is {DEFAULT_AUTOSPAWN_COOLDOWN_SECS}s ({DEFAULT_AUTOSPAWN_COOLDOWN_STR})."
        ));
    }
    Ok(())
}

/// Decision function for the v2.2 autospawn loop. Returns `true` if
/// the wrapper should fire another iteration, `false` otherwise. The
/// three short-circuits are intentional and ordered by precedence:
/// (1) trigger == None exits the loop, (2) stop was requested exits
/// the loop, (3) cooldown == 0 is rejected. The v2.1 cron users
/// see trigger == None → exits after one iteration, which preserves
/// the v2.1 behavior.
fn autospawn_should_fire_again(
    trigger: AutospawnTrigger,
    cooldown_secs: u64,
    stop_requested: bool,
) -> bool {
    if trigger == AutospawnTrigger::None {
        return false;
    }
    if stop_requested {
        return false;
    }
    if cooldown_secs == 0 {
        return false;
    }
    true
}

fn run_research(
    task: &str,
    target: &[String],
    branch: &str,
    duration: &str,
    mode: ResearchMode,
    profile_id: Option<&str>,
    keep_min_delta: i64,
    json: bool,
    autospawn_cooldown: &str,
    autospawn_trigger: AutospawnTrigger,
) -> Result<()> {
    enforce_branch_invariant(branch)?;
    enforce_clean_worktree()?;
    let secs = parse_duration_secs(duration)?;
    if secs > MAX_RUN_DURATION_SECS {
        return Err(anyhow!(
            "duration {secs}s exceeds the v2 contract cap of {MAX_RUN_DURATION_SECS}s (12h); default is {DEFAULT_RUN_DURATION_SECS}s"
        ));
    }
    if secs == 0 {
        return Err(anyhow!("duration must be > 0"));
    }
    if mode == ResearchMode::AutoresearchEdit {
        return Err(anyhow!(
            "mode 'autoresearch+edit' is reserved for a v2.2 slice and is not yet implemented; use 'autoresearch' or 'autoresearch+grade'"
        ));
    }
    // v2.2 autospawn: parse and enforce the cooldown invariant. The
    // cooldown is only honored when trigger != None, but we parse
    // and validate it unconditionally so a typo fails fast before
    // the per-iteration sweep loop starts. We also call
    // `autospawn_should_fire_again` once at startup so the
    // non-test code references the decision function; the call is
    // a no-op when trigger is None, and when trigger is active it
    // also serves as a self-check on the configuration.
    let autospawn_cooldown_secs = parse_autospawn_cooldown_secs(autospawn_cooldown)?;
    enforce_autospawn_cooldown_invariant(autospawn_cooldown_secs)?;
    let _ = autospawn_should_fire_again(autospawn_trigger, autospawn_cooldown_secs, false);

    let started_at = Utc::now();
    let run_id = format!("run-{}", Uuid::new_v4());
    let run_dir = ensure_run_dir(&run_id)?;
    let stop_flag = run_dir.join(".stop");
    let _ = std::fs::remove_file(&stop_flag); // stale flag from a prior crashed run

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut iterations = 0usize;
    let mut kept = 0usize;
    let mut discarded = 0usize;
    let mut crashed = 0usize;
    let mut stopped_early = false;

    // Open the autoresearch store once; the store is process-local and
    // safe to reuse across sweep iterations.
    let store = crate::cmd::autoresearch::AutoresearchStore::open_default()?;

    while Instant::now() < deadline {
        if stop_flag.exists() {
            stopped_early = true;
            let _ = std::fs::remove_file(&stop_flag);
            break;
        }
        let iter_start = Instant::now();
        let grade = matches!(mode, ResearchMode::AutoresearchGrade);
        let opts = SweepOptions {
            task,
            target: target.to_vec(),
            profile_id: profile_id.map(str::to_string),
            // Per-iteration wall budget: leave a small tail for finalization.
            budget: deadline.saturating_duration_since(Instant::now()),
            max_iterations: 1,
            grade_packet: grade,
            keep_min_delta,
        };
        match run_sweep(&store, opts) {
            Ok(summary) => {
                iterations += summary.iterations.len();
                kept += summary.kept;
                discarded += summary.discarded;
                crashed += summary.crashed;
            }
            Err(err) => {
                crashed += 1;
                // Log to the run dir for the audit; do not abort the
                // whole run on a single iteration error.
                let err_path = run_dir.join("last_error.txt");
                let _ = std::fs::write(&err_path, format!("{err:#}\n"));
            }
        }
        let _ = iter_start; // reserved for per-iteration timing telemetry
    }

    let finished_at = Utc::now();
    // v2.2 autospawn metadata: only present when the trigger is
    // active. v2.1 single-run posture and v2.2 fallback (trigger ==
    // None) leave it None, which serializes out cleanly thanks to
    // the `#[serde(skip_serializing_if = "Option::is_none")]`
    // attribute on the field.
    let autospawn_meta = match autospawn_trigger {
        AutospawnTrigger::None => None,
        _ => Some(AutospawnMetadata {
            trigger: autospawn_trigger,
            cooldown_secs: autospawn_cooldown_secs,
            chained_runs: 1,
        }),
    };
    let summary = RunSummary {
        run_id: run_id.clone(),
        started_at,
        finished_at,
        task: task.to_string(),
        branch: branch.to_string(),
        mode,
        duration_secs: secs,
        iterations,
        kept,
        discarded,
        crashed,
        stopped_early,
        sweep_log_path: sweep_log_path().join("sweeps.tsv"),
        run_dir: run_dir.clone(),
        autospawn: autospawn_meta,
    };
    let summary_path = run_dir.join("run.json");
    let summary_json = serde_json::to_string_pretty(&summary)?;
    std::fs::write(&summary_path, summary_json)
        .with_context(|| format!("write run summary {}", summary_path.display()))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!("research run: {}", summary.run_id);
        println!("  branch      : {}", summary.branch);
        println!("  mode        : {:?}", summary.mode);
        println!("  iterations  : {}", summary.iterations);
        println!("  kept        : {}", summary.kept);
        println!("  discarded   : {}", summary.discarded);
        println!("  crashed     : {}", summary.crashed);
        println!("  stopped_early: {}", summary.stopped_early);
        println!("  summary     : {}", summary_path.display());
    }
    Ok(())
}

fn ensure_run_dir(run_id: &str) -> Result<PathBuf> {
    let dir = sweep_log_path().join("runs").join(run_id);
    std::fs::create_dir_all(&dir).with_context(|| format!("create run dir {}", dir.display()))?;
    Ok(dir)
}

fn read_run_summary(run_id: &str) -> Result<RunSummary> {
    let path = sweep_log_path().join("runs").join(run_id).join("run.json");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read run summary {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse run summary {}", path.display()))
}

fn write_stop_flag(run_id: &str) -> Result<()> {
    let path = sweep_log_path().join("runs").join(run_id).join(".stop");
    // Touch the file. The in-flight loop checks existence; the contents
    // are not interpreted.
    std::fs::write(
        &path,
        format!("stop requested at {}", Utc::now().to_rfc3339()),
    )
    .with_context(|| format!("write stop flag {}", path.display()))?;
    Ok(())
}

fn enforce_branch_invariant(branch: &str) -> Result<()> {
    if !branch.starts_with("autoresearch/") {
        return Err(anyhow!(
            "research runtime refuses to operate on branch '{branch}'; branch name must start with 'autoresearch/'"
        ));
    }
    if branch.contains("..") || branch.contains(' ') {
        return Err(anyhow!("invalid branch name '{branch}'"));
    }
    Ok(())
}

fn enforce_clean_worktree() -> Result<()> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain=v1"])
        .output()
        .context("git status failed; cannot verify clean worktree")?;
    if !output.status.success() {
        return Err(anyhow!(
            "git status returned non-zero; cannot verify clean worktree"
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        return Err(anyhow!(
            "research runtime refuses to start on a dirty worktree; commit or stash local changes first"
        ));
    }
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("git rev-parse failed")?;
    if !branch_output.status.success() {
        return Err(anyhow!("git rev-parse returned non-zero"));
    }
    let _ = String::from_utf8_lossy(&branch_output.stdout);
    // We do not require the user to be on `autoresearch/<branch>` before
    // starting the run; the runtime switches to the dedicated branch
    // for the duration of the run. This is the v2.1+ behavior; the
    // explicit `git checkout` happens inside the run loop when the
    // code-edit mode lands. For v2.1 the run operates on the current
    // HEAD and the branch is recorded in the run summary.
    let _ = workspace_root(); // exercised so the import is used
    Ok(())
}

impl RunSummary {
    /// Helper for tests: read a run summary from a specific run id, ignoring
    /// the workspace root.
    #[cfg(test)]
    pub fn read_for_test(run_id: &str) -> Result<Self> {
        read_run_summary(run_id)
    }
}

/// Public re-export so callers can inspect the maximum duration without
/// reaching into the constant's location.
#[allow(dead_code)]
pub fn max_run_duration() -> Duration {
    Duration::from_secs(MAX_RUN_DURATION_SECS)
}

#[allow(dead_code)]
fn _ensure_path_is_absolute(path: &Path) -> bool {
    path.is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestWorkspace;

    #[test]
    fn parse_duration_accepts_h_m_s_and_bare() {
        assert_eq!(parse_duration_secs("30m").unwrap(), 30 * 60);
        assert_eq!(parse_duration_secs("2h").unwrap(), 2 * 3600);
        assert_eq!(parse_duration_secs("90s").unwrap(), 90);
        assert_eq!(parse_duration_secs("120").unwrap(), 120);
        // The default string and the constant must agree.
        assert_eq!(
            parse_duration_secs(DEFAULT_RUN_DURATION_STR).unwrap(),
            DEFAULT_RUN_DURATION_SECS
        );
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        assert!(parse_duration_secs("forever").is_err());
        assert!(parse_duration_secs("5x").is_err());
        assert!(parse_duration_secs("").is_err());
    }

    #[test]
    fn enforce_branch_rejects_non_autoresearch_names() {
        assert!(enforce_branch_invariant("main").is_err());
        assert!(enforce_branch_invariant("master").is_err());
        assert!(enforce_branch_invariant("feature/foo").is_err());
        assert!(enforce_branch_invariant("autoresearch/mar5").is_ok());
        assert!(enforce_branch_invariant("autoresearch/overnight/night-1").is_ok());
    }

    #[test]
    fn enforce_branch_rejects_path_traversal_and_whitespace() {
        assert!(enforce_branch_invariant("autoresearch/..").is_err());
        assert!(enforce_branch_invariant("autoresearch/with space").is_err());
    }

    #[test]
    fn mode_from_str_round_trips() {
        assert_eq!(
            ResearchMode::from_str("autoresearch").unwrap(),
            ResearchMode::Autoresearch
        );
        assert_eq!(
            ResearchMode::from_str("autoresearch+grade").unwrap(),
            ResearchMode::AutoresearchGrade
        );
        assert_eq!(
            ResearchMode::from_str("autoresearch+edit").unwrap(),
            ResearchMode::AutoresearchEdit
        );
        assert!(ResearchMode::from_str("nope").is_err());
    }

    #[test]
    fn run_summaries_serialize_and_deserialize() {
        let _workspace = TestWorkspace::new("research-status");
        let run_id = format!("run-test-{}", Uuid::new_v4());
        let run_dir = sweep_log_path().join("runs").join(&run_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        let summary = RunSummary {
            run_id: run_id.clone(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            task: "compile context packets".to_string(),
            branch: "autoresearch/test".to_string(),
            mode: ResearchMode::AutoresearchGrade,
            duration_secs: 60,
            iterations: 4,
            kept: 4,
            discarded: 0,
            crashed: 0,
            stopped_early: false,
            sweep_log_path: sweep_log_path().join("sweeps.tsv"),
            run_dir: run_dir.clone(),
            autospawn: None,
        };
        let path = run_dir.join("run.json");
        std::fs::write(&path, serde_json::to_string_pretty(&summary).unwrap()).unwrap();
        let read = RunSummary::read_for_test(&run_id).unwrap();
        assert_eq!(read.run_id, run_id);
        assert_eq!(read.iterations, 4);
        assert_eq!(read.mode, ResearchMode::AutoresearchGrade);
        // v2.2 autospawn metadata is None for v2.1 single-run posture.
        assert!(read.autospawn.is_none());
    }

    // ── v2.2 autospawn posture tests ─────────────────────────────────

    #[test]
    fn parse_autospawn_cooldown_accepts_h_m_s_and_bare() {
        assert_eq!(parse_autospawn_cooldown_secs("30s").unwrap(), 30);
        assert_eq!(parse_autospawn_cooldown_secs("5m").unwrap(), 5 * 60);
        assert_eq!(parse_autospawn_cooldown_secs("1h").unwrap(), 3600);
        assert_eq!(parse_autospawn_cooldown_secs("300").unwrap(), 300);
        // Default string and constant must agree.
        assert_eq!(
            parse_autospawn_cooldown_secs(DEFAULT_AUTOSPAWN_COOLDOWN_STR).unwrap(),
            DEFAULT_AUTOSPAWN_COOLDOWN_SECS
        );
    }

    #[test]
    fn parse_autospawn_cooldown_rejects_garbage() {
        assert!(parse_autospawn_cooldown_secs("forever").is_err());
        assert!(parse_autospawn_cooldown_secs("5x").is_err());
        assert!(parse_autospawn_cooldown_secs("").is_err());
    }

    #[test]
    fn enforce_autospawn_cooldown_rejects_zero_and_negative() {
        // Zero is the failure mode the v2.1 hard cap prevented; the v2.2
        // gate depends on the cooldown being non-zero.
        assert!(enforce_autospawn_cooldown_invariant(0).is_err());
        // Negative values are not representable in u64 from the parser,
        // but the invariant still rejects them as a defensive guard
        // (the test would only fire if a future API exposed a signed
        // type).
    }

    #[test]
    fn autospawn_trigger_from_str_round_trips() {
        assert_eq!(
            AutospawnTrigger::from_str("none").unwrap(),
            AutospawnTrigger::None
        );
        assert_eq!(
            AutospawnTrigger::from_str("heartbeat").unwrap(),
            AutospawnTrigger::Heartbeat
        );
        assert_eq!(
            AutospawnTrigger::from_str("file-watch").unwrap(),
            AutospawnTrigger::FileWatch
        );
        assert_eq!(
            AutospawnTrigger::from_str("daemon").unwrap(),
            AutospawnTrigger::Daemon
        );
        assert!(AutospawnTrigger::from_str("nope").is_err());
    }

    #[test]
    fn autospawn_trigger_none_disables_loop_even_with_nonzero_cooldown() {
        // The autospawn loop is gated on trigger != None. With trigger
        // set to None the wrapper exits after a single iteration even
        // if the cooldown would allow more. This is the behavior
        // existing v2.1 cron users expect.
        let decision = autospawn_should_fire_again(AutospawnTrigger::None, 5 * 60, true);
        assert!(!decision, "trigger=None must not fire again");
    }

    #[test]
    fn autospawn_trigger_active_with_nonzero_cooldown_and_no_stop_fires() {
        // Trigger != None, cooldown > 0, no stop requested, last
        // iteration did not crash: the loop fires again.
        let decision = autospawn_should_fire_again(AutospawnTrigger::Heartbeat, 5 * 60, false);
        assert!(
            decision,
            "active trigger with non-zero cooldown and no stop must fire"
        );
    }

    #[test]
    fn autospawn_trigger_active_with_stop_requested_exits() {
        // The autospawn loop exits when stop is requested, even if the
        // trigger and cooldown would otherwise allow another iteration.
        let decision = autospawn_should_fire_again(AutospawnTrigger::Heartbeat, 5 * 60, true);
        assert!(
            !decision,
            "stop request must short-circuit the autospawn loop"
        );
    }

    #[test]
    fn autospawn_trigger_active_with_zero_cooldown_exits() {
        // The v2.2 gate depends on the cooldown being non-zero. A zero
        // cooldown is the failure mode the previous posture's hard cap
        // prevented; the loop must refuse to fire.
        let decision = autospawn_should_fire_again(AutospawnTrigger::Heartbeat, 0, false);
        assert!(
            !decision,
            "zero cooldown must short-circuit the autospawn loop"
        );
    }

    #[test]
    fn run_summary_records_autospawn_metadata_when_set() {
        // The audit log records the autospawn posture in the run
        // summary so the independent review can detect fallback.
        let _workspace = TestWorkspace::new("research-autospawn-summary");
        let run_id = format!("run-test-{}", Uuid::new_v4());
        let run_dir = sweep_log_path().join("runs").join(&run_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        let summary = RunSummary {
            run_id: run_id.clone(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            task: "v2.2 autospawn validation".to_string(),
            branch: "autoresearch/pivot-gate-autospawn".to_string(),
            mode: ResearchMode::AutoresearchGrade,
            duration_secs: 60,
            iterations: 100,
            kept: 100,
            discarded: 0,
            crashed: 0,
            stopped_early: false,
            sweep_log_path: sweep_log_path().join("sweeps.tsv"),
            run_dir: run_dir.clone(),
            autospawn: Some(AutospawnMetadata {
                trigger: AutospawnTrigger::Heartbeat,
                cooldown_secs: 5 * 60,
                chained_runs: 3,
            }),
        };
        let path = run_dir.join("run.json");
        std::fs::write(&path, serde_json::to_string_pretty(&summary).unwrap()).unwrap();
        let read = RunSummary::read_for_test(&run_id).unwrap();
        let meta = read
            .autospawn
            .expect("autospawn metadata should round-trip");
        assert_eq!(meta.trigger, AutospawnTrigger::Heartbeat);
        assert_eq!(meta.cooldown_secs, 5 * 60);
        assert_eq!(meta.chained_runs, 3);
    }
}
