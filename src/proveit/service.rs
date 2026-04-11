use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde_json::json;

use super::artifact_store::ArtifactStore;
use super::git;
use super::manifest;
use super::runner;
use super::types::{
    Cli, CommandKind, FeatureManifest, FeatureVerdict, ProofCategory, ProofOutcome, ProofSpec,
    ReportOutput,
};

pub fn run(cli: Cli) -> Result<()> {
    let workspace_root = detect_workspace_root()?;
    let store = ArtifactStore::new(&workspace_root);

    match cli.command {
        CommandKind::Verify { feature } => {
            let verdict = verify_feature(&workspace_root, &store, &feature)?;
            emit_report(&workspace_root, cli.json, vec![verdict])?;
        }
        CommandKind::Enforce { feature } => {
            let verdict = verify_feature(&workspace_root, &store, &feature)?;
            let may_close = verdict.may_close;
            emit_report(&workspace_root, cli.json, vec![verdict])?;
            if !may_close {
                bail!("feature {feature} does not meet its proof gate");
            }
        }
        CommandKind::Report { feature } => {
            let verdicts = if let Some(feature_id) = feature {
                let v = vec![report_feature(&workspace_root, &store, &feature_id)?];
                emit_report(&workspace_root, cli.json, v)?;
                return Ok(());
            } else {
                let all_verdicts = report_all(&workspace_root, &store)?;
                emit_report(&workspace_root, cli.json, all_verdicts.clone())?;
                all_verdicts
            };
            // Full-run strict gate: every feature must be 5/5 and closable.
            if let Some(failed) = verdicts.iter().find(|v| !v.strict) {
                bail!(
                    "strict gate failed: feature `{}` is not 5/5 (score {}/{} may_close={})",
                    failed.feature_id,
                    failed.score,
                    failed.max_score,
                    failed.may_close,
                );
            }
        }
        CommandKind::VerifyImpacted => {
            let verdicts = verify_impacted(&workspace_root, &store)?;
            emit_report(&workspace_root, cli.json, verdicts.clone())?;
            // Every impacted feature must be strict (5/5 and closable).
            if let Some(failed) = verdicts.iter().find(|v| !v.strict) {
                bail!(
                    "strict gate failed: feature `{}` is not 5/5 (score {}/{} may_close={})",
                    failed.feature_id,
                    failed.score,
                    failed.max_score,
                    failed.may_close,
                );
            }
        }
        CommandKind::Status {
            verbose,
            json,
            warn_only,
        } => {
            status_all(
                &workspace_root,
                &store,
                verbose,
                json || cli.json,
                warn_only,
            )?;
        }
    }

    Ok(())
}

fn verify_feature(
    workspace_root: &Path,
    store: &ArtifactStore,
    feature_id: &str,
) -> Result<FeatureVerdict> {
    let manifest = manifest::load_manifest(workspace_root, feature_id)?;
    let _lock = ProofLock::acquire(workspace_root)?;

    for proof in &manifest.proofs {
        let record = runner::run_proof(workspace_root, &manifest.feature.id, proof)?;
        store.write_proof(&record)?;
    }

    let verdict = compute_verdict(workspace_root, store, &manifest)?;
    store.write_report(&manifest.feature.id, &verdict)?;
    Ok(verdict)
}

fn report_feature(
    workspace_root: &Path,
    store: &ArtifactStore,
    feature_id: &str,
) -> Result<FeatureVerdict> {
    let manifest = manifest::load_manifest(workspace_root, feature_id)?;
    let verdict = compute_verdict(workspace_root, store, &manifest)?;
    store.write_report(&manifest.feature.id, &verdict)?;
    Ok(verdict)
}

fn report_all(workspace_root: &Path, store: &ArtifactStore) -> Result<Vec<FeatureVerdict>> {
    let manifests = manifest::load_all_manifests(workspace_root)?;
    let mut verdicts = Vec::new();
    for item in manifests {
        let verdict = compute_verdict(workspace_root, store, &item)?;
        store.write_report(&item.feature.id, &verdict)?;
        verdicts.push(verdict);
    }
    Ok(verdicts)
}

fn status_all(
    workspace_root: &Path,
    store: &ArtifactStore,
    verbose: bool,
    as_json: bool,
    warn_only: bool,
) -> Result<()> {
    let verdicts_dir = workspace_root.join(".proveit").join("verdicts");
    if !verdicts_dir.exists() {
        if as_json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "features": [] }))?
            );
        } else {
            println!("No cached verdicts found. Run `proveit verify <feature>` first.");
        }
        return Ok(());
    }

    let mut verdicts: Vec<FeatureVerdict> = Vec::new();
    for entry in fs::read_dir(&verdicts_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        match serde_json::from_str::<FeatureVerdict>(&raw) {
            Ok(verdict) => verdicts.push(verdict),
            Err(err) => {
                eprintln!("warning: skipping {}: {err}", path.display());
            }
        }
    }
    verdicts.sort_by(|a, b| a.feature_id.cmp(&b.feature_id));

    // Re-check staleness against current worktree state
    let manifests = manifest::load_all_manifests(workspace_root).unwrap_or_default();
    for verdict in &mut verdicts {
        if let Some(m) = manifests
            .iter()
            .find(|m| m.feature.id == verdict.feature_id)
        {
            let refreshed = compute_verdict(workspace_root, store, m);
            if let Ok(fresh) = refreshed {
                verdict.stale = fresh.stale;
                verdict.changed_files = fresh.changed_files;
            }
        }
    }

    if as_json {
        let output = json!({
            "features": verdicts.iter().map(|v| json!({
                "feature_id": v.feature_id,
                "score": v.score,
                "max_score": v.max_score,
                "stale": v.stale,
                "strict": v.strict,
                "may_close": v.may_close,
                "owner": v.owner,
                "missing_categories": v.missing_categories.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
                "changed_files": v.changed_files,
            })).collect::<Vec<_>>()
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if verdicts.is_empty() {
            println!("No cached verdicts found.");
            return Ok(());
        }
        println!(
            "{:<30} {:>5}  {:>5}  {:>6}  {:>6}",
            "FEATURE", "SCORE", "MAX", "STALE", "STRICT"
        );
        println!("{}", "-".repeat(62));
        for v in &verdicts {
            let stale_marker = if v.stale { "YES" } else { "-" };
            let strict_marker = if v.strict { "YES" } else { "-" };
            println!(
                "{:<30} {:>3}/{:<3}  {:>3}    {:<6}  {:<6}",
                v.feature_id, v.score, v.max_score, v.max_score, stale_marker, strict_marker
            );
        }

        let stale_count = verdicts.iter().filter(|v| v.stale).count();
        let failing_count = verdicts.iter().filter(|v| !v.may_close).count();
        if stale_count > 0 || failing_count > 0 {
            eprintln!();
            if stale_count > 0 {
                eprintln!("warning: {stale_count} feature(s) have stale verdicts");
            }
            if failing_count > 0 {
                eprintln!("warning: {failing_count} feature(s) do not meet their proof gate");
            }
        }

        if verbose {
            println!();
            for v in &verdicts {
                if v.stale || !v.may_close {
                    println!("  {} — {}", v.feature_id, v.recommended_gate_command);
                    if !v.missing_categories.is_empty() {
                        let cats: Vec<&str> =
                            v.missing_categories.iter().map(|c| c.as_str()).collect();
                        println!("    missing: {}", cats.join(", "));
                    }
                    if !v.changed_files.is_empty() {
                        println!("    changed: {}", v.changed_files.join(", "));
                    }
                }
            }
        }
    }

    if !warn_only {
        let any_problems = verdicts.iter().any(|v| v.stale || !v.may_close);
        if any_problems {
            bail!("one or more features have stale or failing verdicts");
        }
    }

    Ok(())
}

fn verify_impacted(workspace_root: &Path, store: &ArtifactStore) -> Result<Vec<FeatureVerdict>> {
    let manifests = manifest::load_all_manifests(workspace_root)?;
    let worktree = git::worktree_changed_files(workspace_root)?;
    let parent = git::parent_changed_files(workspace_root)?;
    let mut changed_set = BTreeSet::new();
    for f in worktree {
        changed_set.insert(f);
    }
    for f in parent {
        changed_set.insert(f);
    }
    let changed_files: Vec<String> = changed_set.into_iter().collect();
    if changed_files.is_empty() {
        return Ok(Vec::new());
    }

    let mut impacted = Vec::new();
    for manifest in manifests {
        if !matching_changed_files(&manifest.feature.watch_paths, &changed_files)?.is_empty() {
            impacted.push(manifest);
        }
    }

    let _lock = ProofLock::acquire(workspace_root)?;
    let mut verdicts = Vec::new();
    for manifest in impacted {
        for proof in &manifest.proofs {
            let record = runner::run_proof(workspace_root, &manifest.feature.id, proof)?;
            store.write_proof(&record)?;
        }
        let verdict = compute_verdict(workspace_root, store, &manifest)?;
        store.write_report(&manifest.feature.id, &verdict)?;
        verdicts.push(verdict);
    }
    Ok(verdicts)
}

fn compute_verdict(
    workspace_root: &Path,
    store: &ArtifactStore,
    manifest: &FeatureManifest,
) -> Result<FeatureVerdict> {
    let mut changed_files = BTreeSet::new();
    let mut proofs = Vec::new();
    let mut categories = BTreeSet::new();

    for proof in &manifest.proofs {
        let outcome = proof_outcome(
            workspace_root,
            store,
            &manifest.feature.watch_paths,
            proof,
            &manifest.feature.id,
        )?;
        for file in &outcome.matched_changes {
            changed_files.insert(file.clone());
        }
        if outcome.passed && !outcome.stale {
            categories.insert(outcome.category);
        }
        proofs.push(outcome);
    }

    let max_score = 5;
    let verdict_score = categories.len() as u8;
    let missing_categories = all_categories()
        .into_iter()
        .filter(|category| !categories.contains(category))
        .collect::<Vec<_>>();
    let stale = proofs.iter().any(|proof| proof.stale);
    let may_close = verdict_score >= manifest.feature.required_score && !stale;
    // strict = 5/5 AND closable: every category present, passing, and no stale proofs.
    // This is the gate for aggregate multi-feature commands (report, verify-impacted).
    let strict = verdict_score == max_score && may_close;

    Ok(FeatureVerdict {
        feature_id: manifest.feature.id.clone(),
        title: manifest.feature.title.clone(),
        owner: manifest.feature.owner.clone(),
        pm_task_id: manifest.feature.pm_task_id.clone(),
        required_score: manifest.feature.required_score,
        score: verdict_score,
        max_score,
        may_close,
        strict,
        stale,
        missing_categories,
        changed_files: changed_files.into_iter().collect(),
        watched_paths: manifest.feature.watch_paths.clone(),
        proofs,
        recommended_gate_command: recommended_gate_command(&manifest.feature.id),
    })
}

fn proof_outcome(
    workspace_root: &Path,
    store: &ArtifactStore,
    watch_paths: &[String],
    proof: &ProofSpec,
    feature_id: &str,
) -> Result<ProofOutcome> {
    let record = store.latest_proof(feature_id, &proof.id)?;
    let mut matched_changes = Vec::new();
    let mut stale = false;
    let mut passed = false;
    let mut exit_code = None;
    let mut commit_sha = None;
    let mut timestamp = None;
    let mut duration_ms = None;
    let mut artifact_present = false;
    let mut artifact_error = None;

    if let Some(record) = record {
        matched_changes = matching_changed_files(
            watch_paths,
            &git::current_changed_files(workspace_root, &record.commit_sha)?,
        )?;
        stale = !matched_changes.is_empty();
        passed = record.passed;
        exit_code = Some(record.exit_code);
        commit_sha = Some(record.commit_sha);
        timestamp = Some(record.timestamp);
        duration_ms = Some(record.duration_ms);
        artifact_present = record.artifact.is_some();
        artifact_error = record.artifact_error;
    }

    Ok(ProofOutcome {
        proof_id: proof.id.clone(),
        category: proof.category,
        description: proof.description.clone(),
        passed,
        stale,
        exit_code,
        commit_sha,
        timestamp,
        duration_ms,
        matched_changes,
        artifact_present,
        artifact_error,
    })
}

fn detect_workspace_root() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("LAYERS_WORKSPACE_ROOT") {
        return Ok(PathBuf::from(root));
    }

    let cwd = std::env::current_dir().context("failed to determine current directory")?;
    find_git_root(&cwd).ok_or_else(|| anyhow::anyhow!("not inside a git repository"))
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn emit_report(workspace_root: &Path, as_json: bool, features: Vec<FeatureVerdict>) -> Result<()> {
    let report = ReportOutput {
        workspace_root: workspace_root.display().to_string(),
        features,
    };

    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    if report.features.is_empty() {
        println!("No matching features.");
        return Ok(());
    }

    for feature in &report.features {
        println!(
            "{} [{}] score {}/{} required {} may_close={} stale={} strict={}",
            feature.feature_id,
            feature.owner,
            feature.score,
            feature.max_score,
            feature.required_score,
            feature.may_close,
            feature.stale,
            feature.strict
        );
        if !feature.changed_files.is_empty() {
            println!("  changed: {}", feature.changed_files.join(", "));
        }
        if !feature.missing_categories.is_empty() {
            let categories = feature
                .missing_categories
                .iter()
                .map(|category| category.as_str())
                .collect::<Vec<_>>();
            println!("  missing: {}", categories.join(", "));
        }
        println!("  gate_command: {}", feature.recommended_gate_command);

        for proof in &feature.proofs {
            let status = if proof.passed && !proof.stale {
                "pass"
            } else if proof.stale {
                "stale"
            } else {
                "fail"
            };
            let commit = proof.commit_sha.as_deref().unwrap_or("none");
            println!(
                "  - {} [{}] {} commit={} exit={}",
                proof.proof_id,
                proof.category.as_str(),
                status,
                commit,
                proof
                    .exit_code
                    .map_or_else(|| "n/a".to_string(), |code| code.to_string())
            );
        }
        println!();
    }

    Ok(())
}

fn matching_changed_files(watch_paths: &[String], changed_files: &[String]) -> Result<Vec<String>> {
    if watch_paths.is_empty() || changed_files.is_empty() {
        return Ok(Vec::new());
    }

    let set = compile_globs(watch_paths)?;
    Ok(changed_files
        .iter()
        .filter(|file| set.is_match(file.as_str()))
        .cloned()
        .collect())
}

fn compile_globs(watch_paths: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in watch_paths {
        builder.add(
            Glob::new(pattern)
                .with_context(|| format!("invalid watch path glob pattern {pattern}"))?,
        );
    }
    builder
        .build()
        .context("failed to compile watch path globs")
}

fn all_categories() -> Vec<ProofCategory> {
    vec![
        ProofCategory::Positive,
        ProofCategory::Counterfactual,
        ProofCategory::Artifact,
        ProofCategory::Failure,
        ProofCategory::Repeatability,
    ]
}

fn recommended_gate_command(feature_id: &str) -> String {
    format!("cargo run --bin proveit -- --json enforce {feature_id}")
}

struct ProofLock {
    path: PathBuf,
}

impl ProofLock {
    fn acquire(workspace_root: &Path) -> Result<Self> {
        let directory = workspace_root.join(".proveit");
        fs::create_dir_all(&directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;
        let path = directory.join("proveit.lock");
        let deadline = Instant::now() + lock_wait_timeout();
        let mut file = loop {
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => break file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Instant::now() >= deadline {
                        let holder = fs::read_to_string(&path)
                            .ok()
                            .filter(|text| !text.trim().is_empty())
                            .unwrap_or_else(|| "<empty lock file>".to_string());
                        bail!(
                            "failed to acquire {} within {:?}; existing lock contents: {}",
                            path.display(),
                            lock_wait_timeout(),
                            holder.trim()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to acquire {}", path.display()));
                }
            }
        };
        let payload = json!({
            "pid": std::process::id(),
            "timestamp": chrono::Utc::now(),
        });
        writeln!(file, "{payload}")?;
        Ok(Self { path })
    }
}

fn lock_wait_timeout() -> Duration {
    let seconds = std::env::var("PROVEIT_LOCK_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(30);
    Duration::from_secs(seconds)
}

impl Drop for ProofLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
