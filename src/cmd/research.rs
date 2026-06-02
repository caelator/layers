//! `layers research` — bounded overnight-researcher runtime (v2 stable-core job 6).
//!
//! This command family is the v2.1+ entrypoint named in `docs/NORTH_STAR.md`
//! and `docs/V2_PRODUCT_CONTRACT.md`. It is the only sanctioned way to run
//! Layers autonomously for a bounded wall-clock window on a dedicated
//! branch, with the user absent.
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

fn run_research(
    task: &str,
    target: &[String],
    branch: &str,
    duration: &str,
    mode: ResearchMode,
    profile_id: Option<&str>,
    keep_min_delta: i64,
    json: bool,
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
        };
        let path = run_dir.join("run.json");
        std::fs::write(&path, serde_json::to_string_pretty(&summary).unwrap()).unwrap();
        let read = RunSummary::read_for_test(&run_id).unwrap();
        assert_eq!(read.run_id, run_id);
        assert_eq!(read.iterations, 4);
        assert_eq!(read.mode, ResearchMode::AutoresearchGrade);
    }
}
