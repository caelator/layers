use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{
    canonical_curated_memory_path, council_files, memoryport_dir, uc_config_path, workspace_root,
};
use crate::council::{
    CouncilRunRequest, execute_council_run, load_council_convergence_record,
    load_council_run_record,
};
use crate::router::{self, Confidence, Route};
use crate::types::{
    CuratedImportRecord, Decision, GraphContext, ProjectRecord, ProjectRecordPayload,
};
use crate::util::{append_jsonl, compact, iso_now, load_jsonl, run_command, which};

// ---------------------------------------------------------------------------
// remember
// ---------------------------------------------------------------------------

pub fn handle_remember(
    kind: &str,
    task: Option<String>,
    task_type: Option<String>,
    summary: Option<String>,
    file: Option<String>,
    artifacts_dir: Option<String>,
    targets: Option<String>,
) -> Result<()> {
    let record = match kind {
        "plan" => {
            let task = task.context("--task required for kind=plan")?;
            let file = file.context("--file required for kind=plan")?;
            let plan_markdown = fs::read_to_string(&file)
                .with_context(|| format!("failed to read plan file: {}", file))?;
            json!({
                "timestamp": iso_now(),
                "task_type": task_type.unwrap_or_else(|| "architecture".to_string()),
                "task": task,
                "plan_markdown": plan_markdown,
                "summary": summary,
                "artifacts_dir": artifacts_dir,
                "targets": parse_targets(targets.as_deref()),
            })
        }
        "learning" => json!({
            "timestamp": iso_now(),
            "kind": "manual-learning",
            "summary": summary.context("--summary required for kind=learning")?,
            "task_type": task_type,
        }),
        "trace" => {
            if task.is_none() && summary.is_none() {
                anyhow::bail!("--task or --summary is required for kind=trace");
            }
            json!({
                "timestamp": iso_now(),
                "task": task,
                "summary": summary,
                "task_type": task_type,
            })
        }
        _ => anyhow::bail!("unsupported kind: {}. Valid kinds: plan, learning, trace", kind),
    };
    let path = match kind {
        "plan" => memoryport_dir().join("council-plans.jsonl"),
        "learning" => memoryport_dir().join("council-learnings.jsonl"),
        "trace" => memoryport_dir().join("council-traces.jsonl"),
        _ => unreachable!(),
    };
    append_jsonl(&path, &record)?;
    println!(
        "{}",
        serde_json::to_string(&json!({"ok": true, "kind": kind}))?
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

const MAX_MEMORY_RECORDS: usize = 3;
const MAX_GITNEXUS_FACTS: usize = 5;
const MAX_OUTPUT_WORDS: usize = 1200;

pub fn handle_query(task: &str, json_out: bool) -> Result<()> {
    let route_result = router::classify(task);

    // Low-confidence downgrades to neither (refusal bias)
    let effective_route = if route_result.confidence == Confidence::Low {
        Route::Neither
    } else {
        route_result.route
    };

    let mut evidence_sections: Vec<String> = Vec::new();
    let mut open_uncertainty: Vec<String> = Vec::new();

    // Retrieve memory if routed
    if matches!(effective_route, Route::MemoryOnly | Route::Both) {
        match retrieve_memory(task) {
            Ok(records) if !records.is_empty() => {
                evidence_sections.push(format!("### Memory\n{}", records.join("\n")));
            }
            Ok(_) => {
                open_uncertainty.push("Memory retrieval returned no matching records.".into());
            }
            Err(e) => {
                open_uncertainty.push(format!("Memory retrieval failed: {e}"));
            }
        }
    }

    // Retrieve graph context if routed
    if matches!(effective_route, Route::GraphOnly | Route::Both) {
        match retrieve_graph(task) {
            Ok(facts) if !facts.is_empty() => {
                evidence_sections.push(format!("### GitNexus\n{}", facts.join("\n")));
            }
            Ok(_) => {
                open_uncertainty
                    .push("GitNexus query returned no results. Run `layers refresh` to update the index.".into());
            }
            Err(e) => {
                open_uncertainty.push(format!("GitNexus retrieval failed: {e}"));
            }
        }
    }

    // Enforce word budget
    let evidence_text = evidence_sections.join("\n\n");
    let word_count = evidence_text.split_whitespace().count();
    let (final_evidence, budget_exceeded) = if word_count > MAX_OUTPUT_WORDS {
        open_uncertainty.push(format!(
            "Evidence exceeded {MAX_OUTPUT_WORDS}-word budget ({word_count} words). Truncated."
        ));
        let truncated: String = evidence_text
            .split_whitespace()
            .take(MAX_OUTPUT_WORDS)
            .collect::<Vec<_>>()
            .join(" ");
        (truncated, true)
    } else {
        (evidence_text, false)
    };

    // Audit log
    let audit = json!({
        "timestamp": iso_now(),
        "action": "query",
        "task": task,
        "route": route_result.route.label(),
        "effective_route": effective_route.label(),
        "confidence": format!("{:?}", route_result.confidence).to_lowercase(),
        "scores": route_result.scores,
        "budget_exceeded": budget_exceeded,
        "evidence_words": word_count,
    });
    let audit_path = memoryport_dir().join("layers-audit.jsonl");
    append_jsonl(&audit_path, &audit)?;

    if json_out {
        let output = json!({
            "route": effective_route.label(),
            "confidence": format!("{:?}", route_result.confidence).to_lowercase(),
            "scores": route_result.scores,
            "why_retrieved": route_result.why,
            "why_not_retrieved": route_result.why_not,
            "evidence": final_evidence,
            "open_uncertainty": open_uncertainty,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if matches!(effective_route, Route::Neither) {
        println!("<layers_context>");
        println!("Route: {}", effective_route.label());
        println!("Why Not Retrieved: {}", route_result.why);
        println!("No context injection — task does not warrant retrieval.");
        println!("</layers_context>");
    } else {
        println!("<layers_context>");
        println!("Route: {}", effective_route.label());
        println!("Why Retrieved: {}", route_result.why);
        if !route_result.why_not.is_empty() {
            println!("Why Not Retrieved: {}", route_result.why_not);
        }
        if !final_evidence.is_empty() {
            println!("\nEvidence:");
            println!("{final_evidence}");
        }
        if !open_uncertainty.is_empty() {
            println!("\nOpen Uncertainty:");
            for u in &open_uncertainty {
                println!("- {u}");
            }
        }
        println!("</layers_context>");
    }

    Ok(())
}

fn retrieve_memory(task: &str) -> Result<Vec<String>> {
    let task_lower = task.to_lowercase();
    let mut scored: Vec<(usize, String)> = Vec::new();

    for (kind, path) in council_files() {
        for record in load_jsonl(&path)? {
            let text = record
                .get("summary")
                .or_else(|| record.get("task"))
                .or_else(|| record.get("plan_markdown"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if text.trim().is_empty() {
                continue;
            }

            // Simple relevance: count task words found in the record
            let text_lower = text.to_lowercase();
            let relevance = task_lower
                .split_whitespace()
                .filter(|w| w.len() > 2 && text_lower.contains(w))
                .count();

            let ts = record
                .get("timestamp")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            scored.push((relevance, format!("- [{}][{}] {}", kind, ts, compact(text, 200))));
        }
    }

    // Also search curated memory
    let curated_path = canonical_curated_memory_path();
    if curated_path.exists() {
        for record in load_jsonl(&curated_path)? {
            let summary = record
                .get("payload")
                .and_then(|p| p.get("summary"))
                .or_else(|| record.get("payload").and_then(|p| p.get("title")))
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if summary.trim().is_empty() {
                continue;
            }

            let text_lower = summary.to_lowercase();
            let relevance = task_lower
                .split_whitespace()
                .filter(|w| w.len() > 2 && text_lower.contains(w))
                .count();

            let entity = record
                .get("entity")
                .and_then(|v| v.as_str())
                .unwrap_or("record");
            scored.push((relevance, format!("- [curated/{}] {}", entity, compact(summary, 200))));
        }
    }

    // Sort by relevance descending, take top N
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(scored
        .into_iter()
        .filter(|(score, _)| *score > 0)
        .take(MAX_MEMORY_RECORDS)
        .map(|(_, line)| line)
        .collect())
}

fn retrieve_graph(task: &str) -> Result<Vec<String>> {
    if which("gitnexus").is_none() {
        anyhow::bail!("gitnexus not found in PATH");
    }

    // Determine repo name from workspace root directory name
    let repo_name = workspace_root()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();
    let args = [
        "gitnexus",
        "query",
        task,
        "--limit",
        "5",
        "--repo",
        &repo_name,
    ];
    match run_command(&args, &workspace_root()) {
        Ok((true, stdout, _)) => {
            let trimmed = stdout.trim();
            if trimmed.is_empty() {
                return Ok(vec![]);
            }
            Ok(trimmed
                .lines()
                .take(MAX_GITNEXUS_FACTS)
                .map(|l| format!("- {}", l.trim()))
                .collect())
        }
        Ok((false, _, stderr)) => {
            anyhow::bail!("gitnexus query failed: {}", stderr.trim());
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// council run
// ---------------------------------------------------------------------------

pub fn handle_council_run(
    task: &str,
    gemini_cmd: Option<String>,
    claude_cmd: Option<String>,
    codex_cmd: Option<String>,
    timeout_secs: u64,
    retry_limit: u32,
    artifacts_dir: Option<String>,
    targets: Option<String>,
    json_out: bool,
) -> Result<()> {
    let gemini_cmd = council_command("gemini", gemini_cmd)?;
    let claude_cmd = council_command("claude", claude_cmd)?;
    let codex_cmd = council_command("codex", codex_cmd)?;

    // Gather context directly from MemoryPort and GitNexus — no routing layer.
    let context_text = gather_context(task)?;
    let target_symbols = parse_targets(targets.as_deref());
    let graph_context = build_graph_context(&target_symbols)?;

    let run = execute_council_run(CouncilRunRequest {
        task: task.to_string(),
        route: "direct".to_string(),
        context_text,
        context_json: json!({"route": "direct"}),
        graph_context,
        targets: target_symbols,
        gemini_cmd,
        claude_cmd,
        codex_cmd,
        retry_limit,
        timeout_secs,
        artifacts_dir: artifacts_dir.map(PathBuf::from),
        trace_path_override: None,
    })?;

    if json_out {
        println!("{}", serde_json::to_string_pretty(&run)?);
    } else {
        let convergence = run
            .convergence
            .as_ref()
            .map(|c| format!("{} ({}): {}", c.status, c.reason, c.summary))
            .unwrap_or_else(|| "no convergence record".to_string());
        println!(
            "Council run {} {} ({})\nArtifacts: {}\n{}",
            run.run_id, run.status, run.status_reason, run.artifacts_dir, convergence
        );
        if !run.degraded_reasons.is_empty() {
            println!("Degraded: {}", run.degraded_reasons.join(" | "));
        }
    }
    Ok(())
}

/// Gather context by calling MemoryPort and GitNexus directly.
/// No routing heuristics — just retrieve from both and concatenate.
fn gather_context(task: &str) -> Result<String> {
    let mut sections = Vec::new();

    // MemoryPort semantic retrieval
    let uc_config = uc_config_path();
    if which("uc").is_some() && uc_config.exists() {
        let args = [
            "uc",
            "-c",
            &uc_config.to_string_lossy(),
            "retrieve",
            task,
            "--top-k",
            "5",
        ];
        if let Ok((true, stdout, _)) = run_command(&args, &workspace_root()) {
            let trimmed = stdout.trim();
            if !trimmed.is_empty() {
                sections.push(format!("## MemoryPort\n{}", trimmed));
            }
        }
    }

    // JSONL memory spine (council plans/learnings/traces)
    let mut spine_hits = Vec::new();
    for (kind, path) in council_files() {
        for record in load_jsonl(&path)?.into_iter().rev().take(3) {
            if let Some(summary) = record.get("summary").and_then(|v| v.as_str()) {
                if !summary.trim().is_empty() {
                    spine_hits.push(format!("- [{}] {}", kind, compact(summary, 200)));
                }
            } else if let Some(task_field) = record.get("task").and_then(|v| v.as_str()) {
                spine_hits.push(format!("- [{}] {}", kind, compact(task_field, 200)));
            }
        }
    }
    if !spine_hits.is_empty() {
        sections.push(format!("## Memory Spine\n{}", spine_hits.join("\n")));
    }

    // Curated memory (decisions, constraints)
    let curated_path = canonical_curated_memory_path();
    if curated_path.exists() {
        let mut curated_hits = Vec::new();
        for record in load_jsonl(&curated_path)?.into_iter().rev().take(5) {
            if let Some(payload) = record.get("payload") {
                let entity = record
                    .get("entity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("record");
                let summary = payload
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if !summary.is_empty() {
                    curated_hits.push(format!("- [{}] {}", entity, compact(summary, 200)));
                }
            }
        }
        if !curated_hits.is_empty() {
            sections.push(format!("## Curated Memory\n{}", curated_hits.join("\n")));
        }
    }

    // GitNexus graph query
    if which("gitnexus").is_some() {
        let args = ["gitnexus", "query", task, "--limit", "5"];
        if let Ok((true, stdout, _)) = run_command(&args, &workspace_root()) {
            let trimmed = stdout.trim();
            if !trimmed.is_empty() {
                sections.push(format!("## GitNexus\n{}", compact(trimmed, 800)));
            }
        }
    }

    if sections.is_empty() {
        Ok("No prior context retrieved.".to_string())
    } else {
        Ok(sections.join("\n\n"))
    }
}

// ---------------------------------------------------------------------------
// council promote
// ---------------------------------------------------------------------------

pub fn handle_council_promote(
    run_id: &str,
    project: &str,
    artifacts_dir: Option<String>,
    dry_run: bool,
    json_out: bool,
) -> Result<()> {
    let project_slug = project.trim().to_lowercase().replace(' ', "-");
    if project_slug.is_empty() {
        anyhow::bail!("project slug must not be empty");
    }
    let artifacts_dir_path = artifacts_dir.as_deref().map(PathBuf::from);
    let run = load_council_run_record(run_id, artifacts_dir_path.as_deref())?;
    let convergence = load_council_convergence_record(&run, artifacts_dir_path.as_deref())?;

    if run.status != "completed" {
        anyhow::bail!(
            "run '{}' is not promotable: status={} reason={}",
            run_id,
            run.status,
            run.status_reason
        );
    }
    if convergence.status != "converged" {
        anyhow::bail!(
            "run '{}' is not converged: status={} reason={}",
            run_id,
            convergence.status,
            convergence.reason
        );
    }

    let record = council_promotion_record(&run, &convergence, &project_slug)?;

    // Check for duplicate promotion
    let existing = load_jsonl(&canonical_curated_memory_path())?;
    if existing
        .iter()
        .any(|r| r.get("id").and_then(|v| v.as_str()) == Some(&record.id))
    {
        anyhow::bail!(
            "run '{}' was already promoted as '{}'",
            run_id,
            record.id
        );
    }

    let payload = json!({
        "ok": true,
        "dry_run": dry_run,
        "run_id": run_id,
        "project": record.project,
        "artifacts_dir": run.artifacts_dir,
        "canonical_path": canonical_curated_memory_path(),
        "record": record,
    });

    if !dry_run {
        append_jsonl(
            &canonical_curated_memory_path(),
            &serde_json::to_value(&record)?,
        )?;
    }

    if json_out || dry_run {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!(
            "Promoted council run {} into {} as {}",
            run_id,
            canonical_curated_memory_path().display(),
            record.id
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// curated import
// ---------------------------------------------------------------------------

pub fn handle_curated_import(file: &str) -> Result<()> {
    let path = Path::new(file);
    if !path.exists() {
        anyhow::bail!("file not found: {}", file);
    }
    let (imported, skipped, errors) = import_curated_memory(path)?;
    let ok = errors == 0;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": ok,
            "file": file,
            "canonical_path": canonical_curated_memory_path(),
            "imported": imported,
            "skipped": skipped,
            "parse_errors": errors,
        }))?
    );
    if !ok {
        anyhow::bail!("{} records failed to parse", errors);
    }
    Ok(())
}

fn import_curated_memory(path: &Path) -> Result<(usize, usize, usize)> {
    let raw_lines = fs::read_to_string(path)?;
    let existing = load_jsonl(&canonical_curated_memory_path())?;
    let mut existing_keys: std::collections::BTreeSet<String> = existing
        .iter()
        .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = 0;

    for line in raw_lines.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        let import: CuratedImportRecord = match serde_json::from_value(parsed) {
            Ok(v) => v,
            Err(e) => {
                anyhow::bail!("record parse error: {}", e);
            }
        };
        let record = curated_import_to_record(import)?;
        if !existing_keys.insert(record.id.clone()) {
            skipped += 1;
            continue;
        }
        append_jsonl(
            &canonical_curated_memory_path(),
            &serde_json::to_value(&record)?,
        )?;
        imported += 1;
    }
    Ok((imported, skipped, errors))
}

fn curated_import_to_record(import: CuratedImportRecord) -> Result<ProjectRecord> {
    let entity = match import.kind.as_str() {
        "decision" | "constraint" | "next_step" | "postmortem" => import.kind.as_str(),
        other => anyhow::bail!(
            "unsupported curated import kind: {}. Valid kinds: decision, constraint, next_step, postmortem",
            other
        ),
    };
    let slug = import
        .summary
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
        .collect::<String>()
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase();
    let id = format!("cm_{}_{}", entity, slug);
    let payload = match entity {
        "decision" => ProjectRecordPayload::Decision(Decision {
            slug: slug.clone(),
            title: compact(&import.summary, 96),
            summary: import.summary.clone(),
            rationale: import.rationale,
        }),
        "constraint" => ProjectRecordPayload::Constraint(crate::types::Constraint {
            slug: slug.clone(),
            title: compact(&import.summary, 96),
            summary: import.summary.clone(),
            impact: String::new(),
        }),
        "next_step" => ProjectRecordPayload::NextStep(crate::types::NextStep {
            slug: slug.clone(),
            title: compact(&import.summary, 96),
            summary: import.summary.clone(),
            owner: String::new(),
        }),
        "postmortem" => ProjectRecordPayload::Postmortem(crate::types::Postmortem {
            slug: slug.clone(),
            title: compact(&import.summary, 96),
            summary: import.summary.clone(),
            root_cause: String::new(),
        }),
        _ => unreachable!(),
    };
    Ok(ProjectRecord {
        id,
        entity: entity.to_string(),
        project: import.project,
        task: None,
        created_at: if import.timestamp.is_empty() {
            iso_now()
        } else {
            import.timestamp
        },
        source: "curated-import".to_string(),
        tags: import.tags,
        archived: false,
        metadata: None,
        payload,
    })
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

pub fn handle_validate(routing_benchmarks: Option<String>) -> Result<()> {
    // Check JSONL stores exist
    let spine_files: Vec<_> = council_files()
        .into_iter()
        .map(|(kind, path)| {
            let exists = path.exists();
            let count = if exists {
                load_jsonl(&path).map(|v| v.len()).unwrap_or(0)
            } else {
                0
            };
            json!({"kind": kind, "path": path, "exists": exists, "records": count})
        })
        .collect();

    let curated_path = canonical_curated_memory_path();
    let curated_count = if curated_path.exists() {
        load_jsonl(&curated_path).map(|v| v.len()).unwrap_or(0)
    } else {
        0
    };

    // Check council commands
    let council_configured = [
        "LAYERS_COUNCIL_GEMINI_CMD",
        "LAYERS_COUNCIL_CLAUDE_CMD",
        "LAYERS_COUNCIL_CODEX_CMD",
    ]
    .iter()
    .all(|key| {
        std::env::var(key)
            .ok()
            .is_some_and(|v| !v.trim().is_empty())
    });

    // Check external tools
    let has_uc = which("uc").is_some() && uc_config_path().exists();
    let has_gitnexus = which("gitnexus").is_some();

    let ok = has_uc || has_gitnexus; // at least one retrieval source

    // Run routing benchmarks if requested
    let benchmark_result = if let Some(ref bench_file) = routing_benchmarks {
        Some(run_routing_benchmarks(bench_file)?)
    } else {
        None
    };

    let mut payload = json!({
        "ok": ok,
        "memory_spine": spine_files,
        "curated_memory": {
            "path": curated_path,
            "exists": curated_path.exists(),
            "records": curated_count,
        },
        "council": {
            "commands_configured": council_configured,
            "order": "Gemini -> Claude -> Codex",
        },
        "tools": {
            "uc": has_uc,
            "gitnexus": has_gitnexus,
        },
        "workspace": workspace_root(),
    });

    if let Some(bench) = &benchmark_result {
        payload["routing_benchmarks"] = bench.clone();
        if bench["pass_rate"].as_f64().unwrap_or(0.0) < 1.0 {
            payload["ok"] = json!(false);
        }
    }

    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

/// Run routing benchmarks from an answer-key JSONL file.
///
/// Each line: `{"query": "...", "expected_route": "neither|memory_only|graph_only|both"}`
/// Optional: `"expected_confidence": "high|low"`, `"note": "..."`
fn run_routing_benchmarks(file: &str) -> Result<Value> {
    let path = Path::new(file);
    if !path.exists() {
        anyhow::bail!("benchmark file not found: {}", file);
    }
    let lines = fs::read_to_string(path)?;
    let mut total = 0u32;
    let mut passed = 0u32;
    let mut failures: Vec<Value> = Vec::new();

    for (line_num, line) in lines.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let case: Value = serde_json::from_str(line)
            .with_context(|| format!("parse error on line {}", line_num + 1))?;

        let query = case["query"]
            .as_str()
            .context("missing 'query' field")?;
        let expected_route = case["expected_route"]
            .as_str()
            .context("missing 'expected_route' field")?;

        let result = router::classify(query);

        // Apply refusal bias (same as handle_query)
        let effective_route = if result.confidence == Confidence::Low {
            Route::Neither
        } else {
            result.route
        };

        let route_match = effective_route.label() == expected_route;

        let confidence_match = case
            .get("expected_confidence")
            .and_then(|v| v.as_str())
            .map(|ec| {
                let actual = format!("{:?}", result.confidence).to_lowercase();
                actual == ec
            })
            .unwrap_or(true); // no expectation = pass

        total += 1;
        if route_match && confidence_match {
            passed += 1;
        } else {
            let mut failure = json!({
                "line": line_num + 1,
                "query": query,
                "expected_route": expected_route,
                "actual_route": effective_route.label(),
                "actual_confidence": format!("{:?}", result.confidence).to_lowercase(),
                "scores": result.scores,
            });
            if let Some(ec) = case.get("expected_confidence") {
                failure["expected_confidence"] = ec.clone();
            }
            if let Some(note) = case.get("note") {
                failure["note"] = note.clone();
            }
            failures.push(failure);
        }
    }

    let pass_rate = if total > 0 {
        passed as f64 / total as f64
    } else {
        1.0
    };

    Ok(json!({
        "file": file,
        "total": total,
        "passed": passed,
        "failed": total - passed,
        "pass_rate": pass_rate,
        "failures": failures,
    }))
}

// ---------------------------------------------------------------------------
// refresh
// ---------------------------------------------------------------------------

pub fn handle_refresh(embeddings: bool) -> Result<()> {
    let root = workspace_root();
    let mut results: Vec<Value> = Vec::new();

    // 1. Refresh GitNexus index
    if which("npx").is_some() {
        let mut args = vec!["npx", "gitnexus", "analyze"];
        if embeddings {
            args.push("--embeddings");
        }
        eprintln!("Running: {}", args.join(" "));
        match run_command(&args, &root) {
            Ok((true, stdout, _)) => {
                results.push(json!({
                    "tool": "gitnexus",
                    "status": "ok",
                    "output": compact(stdout.trim(), 500),
                }));
            }
            Ok((false, _, stderr)) => {
                results.push(json!({
                    "tool": "gitnexus",
                    "status": "error",
                    "error": compact(stderr.trim(), 500),
                }));
            }
            Err(e) => {
                results.push(json!({
                    "tool": "gitnexus",
                    "status": "error",
                    "error": e.to_string(),
                }));
            }
        }
    } else {
        results.push(json!({
            "tool": "gitnexus",
            "status": "skipped",
            "reason": "npx not found in PATH",
        }));
    }

    // 2. Refresh MemoryPort: flush pending chunks, then check status
    let has_uc = which("uc").is_some() && uc_config_path().exists();
    if has_uc {
        let uc_cfg = uc_config_path();
        let uc_cfg_str = uc_cfg.to_string_lossy().to_string();

        // Flush buffered chunks (triggers embedding for any pending data)
        let flush_args = ["uc", "-c", &uc_cfg_str, "flush"];
        eprintln!("Running: {}", flush_args.join(" "));
        let flush_result = match run_command(&flush_args, &root) {
            Ok((true, stdout, _)) => {
                json!({"action": "flush", "status": "ok", "output": compact(stdout.trim(), 200)})
            }
            Ok((false, _, stderr)) => {
                json!({"action": "flush", "status": "error", "error": compact(stderr.trim(), 200)})
            }
            Err(e) => {
                json!({"action": "flush", "status": "error", "error": e.to_string()})
            }
        };

        // Check status
        let status_args = ["uc", "-c", &uc_cfg_str, "status"];
        let status_result = match run_command(&status_args, &root) {
            Ok((true, stdout, _)) => {
                json!({"action": "status", "status": "ok", "output": compact(stdout.trim(), 500)})
            }
            Ok((false, _, stderr)) => {
                json!({"action": "status", "status": "error", "error": compact(stderr.trim(), 500)})
            }
            Err(e) => {
                json!({"action": "status", "status": "error", "error": e.to_string()})
            }
        };

        let mp_ok = flush_result["status"] != "error" && status_result["status"] != "error";
        results.push(json!({
            "tool": "memoryport",
            "status": if mp_ok { "ok" } else { "error" },
            "steps": [flush_result, status_result],
        }));
    } else {
        results.push(json!({
            "tool": "memoryport",
            "status": "skipped",
            "reason": if which("uc").is_none() { "uc not found in PATH" } else { "uc.toml not found" },
        }));
    }

    // 3. Verify JSONL stores
    let spine_status: Vec<_> = council_files()
        .into_iter()
        .map(|(kind, path)| {
            let exists = path.exists();
            let count = if exists {
                load_jsonl(&path).map(|v| v.len()).unwrap_or(0)
            } else {
                0
            };
            json!({"kind": kind, "exists": exists, "records": count})
        })
        .collect();
    results.push(json!({
        "tool": "memory_spine",
        "status": "ok",
        "stores": spine_status,
    }));

    let ok = results.iter().all(|r| r["status"] != "error");
    let payload = json!({
        "ok": ok,
        "results": results,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn parse_targets(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn council_command(stage: &str, explicit: Option<String>) -> Result<String> {
    if let Some(cmd) = explicit {
        if !cmd.trim().is_empty() {
            return Ok(cmd);
        }
    }
    let env_key = match stage {
        "gemini" => "LAYERS_COUNCIL_GEMINI_CMD",
        "claude" => "LAYERS_COUNCIL_CLAUDE_CMD",
        "codex" => "LAYERS_COUNCIL_CODEX_CMD",
        _ => anyhow::bail!("unsupported council stage: {}", stage),
    };
    std::env::var(env_key).with_context(|| {
        format!(
            "{} not set; pass --{}-cmd or set the environment variable",
            env_key, stage
        )
    })
}

fn build_graph_context(targets: &[String]) -> Result<Option<GraphContext>> {
    if targets.is_empty() || which("gitnexus").is_none() {
        return Ok(None);
    }
    // Run impact analysis for each target
    let mut all_direct = 0u64;
    let mut all_indirect = 0u64;
    let mut all_transitive = 0u64;
    let mut risk_level = String::new();
    let mut affected_processes = Vec::new();

    for target in targets {
        let args = ["gitnexus", "impact", target, "--direction", "upstream"];
        if let Ok((true, stdout, _)) = run_command(&args, &workspace_root()) {
            // Parse counts from output
            if let Ok(parsed) = serde_json::from_str::<Value>(&stdout) {
                if let Some(d) = parsed.get("direct").and_then(|v| v.as_u64()) {
                    all_direct += d;
                }
                if let Some(i) = parsed.get("indirect").and_then(|v| v.as_u64()) {
                    all_indirect += i;
                }
                if let Some(t) = parsed.get("transitive").and_then(|v| v.as_u64()) {
                    all_transitive += t;
                }
                if let Some(r) = parsed.get("risk_level").and_then(|v| v.as_str()) {
                    risk_level = r.to_string();
                }
                if let Some(procs) = parsed.get("affected_processes").and_then(|v| v.as_array()) {
                    for p in procs {
                        if let Some(s) = p.as_str() {
                            if !affected_processes.contains(&s.to_string()) {
                                affected_processes.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Some(GraphContext {
        impact_summary: Some(crate::types::ImpactSummary {
            target_symbols: targets.to_vec(),
            blast_radius: crate::types::BlastRadius {
                direct: all_direct,
                indirect: all_indirect,
                transitive: all_transitive,
            },
            risk_level,
            affected_processes,
        }),
    }))
}

pub fn council_promotion_record(
    run: &crate::types::CouncilRunRecord,
    convergence: &crate::types::CouncilConvergenceRecord,
    project: &str,
) -> Result<ProjectRecord> {
    let decision_text = convergence.decision.trim();
    if decision_text.is_empty() {
        anyhow::bail!(
            "run '{}' convergence does not contain a structured decision",
            run.run_id
        );
    }
    let slug = run
        .run_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let rationale = if convergence.why.is_empty() {
        format!("Promoted from converged council run {}.", run.run_id)
    } else {
        convergence.why.join(" ")
    };

    Ok(ProjectRecord {
        id: format!("cm_decision_council_{}", slug),
        entity: "decision".to_string(),
        project: project.to_string(),
        task: None,
        created_at: iso_now(),
        source: "council-promotion-v1".to_string(),
        tags: vec!["council".to_string(), "promoted".to_string()],
        archived: false,
        metadata: Some(json!({
            "promotion": {
                "run_id": run.run_id,
                "artifacts_dir": run.artifacts_dir,
                "convergence_status": convergence.status,
            }
        })),
        payload: ProjectRecordPayload::Decision(Decision {
            slug,
            title: compact(decision_text, 96),
            summary: if convergence.summary.trim().is_empty() {
                decision_text.to_string()
            } else {
                convergence.summary.trim().to_string()
            },
            rationale,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestWorkspace;
    use crate::types::{CouncilConvergenceRecord, CouncilRunRecord};
    use crate::util::load_jsonl;
    use serde_json::json;
    use std::fs;

    #[test]
    fn remember_plan_writes_to_council_plans() {
        let ws = TestWorkspace::new("remember-plan");
        let root = ws.root();
        let plan_file = root.join("test-plan.md");
        fs::write(&plan_file, "# Test Plan\nDo the thing.").unwrap();

        handle_remember(
            "plan",
            Some("test-task".to_string()),
            Some("architecture".to_string()),
            None,
            Some(plan_file.to_string_lossy().to_string()),
            None,
            None,
        )
        .unwrap();

        let records = load_jsonl(&root.join("memoryport").join("council-plans.jsonl")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["task"], "test-task");
        assert!(records[0]["plan_markdown"]
            .as_str()
            .unwrap()
            .contains("Do the thing"));
    }

    #[test]
    fn remember_learning_writes_to_council_learnings() {
        let ws = TestWorkspace::new("remember-learning");
        let root = ws.root();

        handle_remember(
            "learning",
            None,
            None,
            Some("Always check convergence before promoting.".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let records =
            load_jsonl(&root.join("memoryport").join("council-learnings.jsonl")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0]["summary"],
            "Always check convergence before promoting."
        );
    }

    #[test]
    fn remember_trace_writes_to_council_traces() {
        let ws = TestWorkspace::new("remember-trace");
        let root = ws.root();

        handle_remember(
            "trace",
            Some("council-run-123".to_string()),
            None,
            Some("Council converged after 2 rounds.".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        let records = load_jsonl(&root.join("memoryport").join("council-traces.jsonl")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["task"], "council-run-123");
    }

    #[test]
    fn remember_rejects_unsupported_kind() {
        let _ws = TestWorkspace::new("remember-bad-kind");
        let result = handle_remember("bogus", None, None, None, None, None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported kind"));
    }

    #[test]
    fn remember_plan_requires_task_and_file() {
        let _ws = TestWorkspace::new("remember-plan-missing");
        let no_task = handle_remember("plan", None, None, None, None, None, None);
        assert!(no_task.is_err());
        let no_file = handle_remember(
            "plan",
            Some("task".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        assert!(no_file.is_err());
    }

    #[test]
    fn curated_import_deduplicates() {
        let ws = TestWorkspace::new("curated-import-dedup");
        let root = ws.root();

        let import_file = root.join("import.jsonl");
        let record = json!({
            "kind": "decision",
            "project": "layers",
            "summary": "Use direct context gathering instead of routing heuristics.",
            "rationale": "Simpler and more predictable.",
            "timestamp": "2026-04-02T00:00:00Z",
            "tags": ["layers"]
        });
        fs::write(
            &import_file,
            format!("{}\n{}\n", record, record),
        )
        .unwrap();

        handle_curated_import(&import_file.to_string_lossy()).unwrap();

        let records =
            load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
        assert_eq!(records.len(), 1, "duplicate should be skipped");
    }

    #[test]
    fn curated_import_rejects_unsupported_kind() {
        let ws = TestWorkspace::new("curated-import-bad");
        let root = ws.root();
        let import_file = root.join("bad-import.jsonl");
        fs::write(
            &import_file,
            json!({"kind": "status", "project": "x", "summary": "y"}).to_string() + "\n",
        )
        .unwrap();
        let result = handle_curated_import(&import_file.to_string_lossy());
        assert!(result.is_err());
    }

    #[test]
    fn council_promotion_record_builds_correct_structure() {
        let _ws = TestWorkspace::new("promote-record");
        let run = CouncilRunRecord {
            run_id: "council-20260402-test".to_string(),
            task: "Test promotion".to_string(),
            status: "completed".to_string(),
            status_reason: "converged".to_string(),
            artifacts_dir: "/tmp/test-artifacts".to_string(),
            route: "direct".to_string(),
            ..Default::default()
        };
        let convergence = CouncilConvergenceRecord {
            status: "converged".to_string(),
            reason: "converged".to_string(),
            decision: "Adopt the lightweight refactor.".to_string(),
            summary: "Lightweight refactor adopted.".to_string(),
            why: vec!["Simpler architecture.".to_string()],
            ..Default::default()
        };
        let record = council_promotion_record(&run, &convergence, "layers").unwrap();
        assert_eq!(record.entity, "decision");
        assert_eq!(record.project, "layers");
        assert_eq!(record.source, "council-promotion-v1");
        assert!(record.id.contains("council-20260402-test"));
    }

    #[test]
    fn council_promotion_rejects_empty_decision() {
        let _ws = TestWorkspace::new("promote-empty");
        let run = CouncilRunRecord {
            run_id: "empty-decision".to_string(),
            ..Default::default()
        };
        let convergence = CouncilConvergenceRecord {
            decision: "  ".to_string(),
            ..Default::default()
        };
        let result = council_promotion_record(&run, &convergence, "layers");
        assert!(result.is_err());
    }

    #[test]
    fn validate_runs_without_benchmarks() {
        let _ws = TestWorkspace::new("validate-no-bench");
        let result = handle_validate(None);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_runs_routing_benchmarks() {
        let ws = TestWorkspace::new("validate-bench");
        let root = ws.root();
        let bench_file = root.join("benchmarks.jsonl");
        fs::write(
            &bench_file,
            concat!(
                r#"{"query": "rename this variable to snake_case", "expected_route": "neither", "expected_confidence": "high"}"#,
                "\n",
                r#"{"query": "hello", "expected_route": "neither", "expected_confidence": "low"}"#,
                "\n",
            ),
        )
        .unwrap();

        let result = handle_validate(Some(bench_file.to_string_lossy().to_string()));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_benchmarks_reports_failures() {
        let ws = TestWorkspace::new("validate-bench-fail");
        let root = ws.root();
        let bench_file = root.join("bad-bench.jsonl");
        // Force a mismatch: expect "both" for a trivial query
        fs::write(
            &bench_file,
            r#"{"query": "rename x to y", "expected_route": "both"}"#,
        )
        .unwrap();

        // validate should still succeed (it reports, doesn't bail)
        let result = handle_validate(Some(bench_file.to_string_lossy().to_string()));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_benchmarks_rejects_missing_file() {
        let _ws = TestWorkspace::new("validate-bench-missing");
        let result = handle_validate(Some("/nonexistent/file.jsonl".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn council_command_prefers_explicit_over_env() {
        let result = council_command("gemini", Some("echo test".to_string()));
        assert_eq!(result.unwrap(), "echo test");
    }

    #[test]
    fn council_command_falls_back_to_env() {
        unsafe {
            std::env::set_var("LAYERS_COUNCIL_GEMINI_CMD", "gemini-cli");
        }
        let result = council_command("gemini", None);
        assert_eq!(result.unwrap(), "gemini-cli");
        unsafe {
            std::env::remove_var("LAYERS_COUNCIL_GEMINI_CMD");
        }
    }

    #[test]
    fn council_command_fails_without_env_or_explicit() {
        unsafe {
            std::env::remove_var("LAYERS_COUNCIL_CODEX_CMD");
        }
        let result = council_command("codex", None);
        assert!(result.is_err());
    }

    #[test]
    fn promote_rejects_incomplete_run() {
        let ws = TestWorkspace::new("promote-incomplete");
        let root = ws.root();

        let artifacts_dir = root
            .join("memoryport")
            .join("council-runs")
            .join("incomplete-run");
        fs::create_dir_all(&artifacts_dir).unwrap();

        let run = json!({
            "run_id": "incomplete-run",
            "task": "test",
            "status": "incomplete",
            "status_reason": "stall",
            "artifacts_dir": artifacts_dir,
        });
        fs::write(
            artifacts_dir.join("run.json"),
            serde_json::to_string_pretty(&run).unwrap(),
        )
        .unwrap();

        let result = handle_council_promote("incomplete-run", "layers", None, false, true);
        assert!(result.is_err(), "expected error for incomplete run");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not promotable") || msg.contains("incomplete"),
            "unexpected error: {}",
            msg
        );
    }

    fn seed_promotable_run(root: &Path, run_id: &str) {
        let artifacts_dir = root
            .join("memoryport")
            .join("council-runs")
            .join(run_id);
        fs::create_dir_all(&artifacts_dir).unwrap();

        let convergence_output = artifacts_dir.join("codex-attempt-1.stdout.txt");
        fs::write(&convergence_output, "## Decision\n- adopt this\n## Why\n- it works\n").unwrap();

        let run = json!({
            "run_id": run_id,
            "task": "test",
            "status": "completed",
            "status_reason": "converged",
            "artifacts_dir": artifacts_dir,
            "route": "direct",
            "targets": [],
            "stages": [],
            "convergence": {
                "status": "converged",
                "reason": "converged",
                "decision": "adopt this",
                "summary": "adopted",
                "why": ["it works"],
                "output_path": convergence_output,
            }
        });
        fs::write(
            artifacts_dir.join("run.json"),
            serde_json::to_string_pretty(&run).unwrap(),
        )
        .unwrap();
        fs::write(
            artifacts_dir.join("convergence.json"),
            serde_json::to_string_pretty(&run["convergence"]).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn promote_writes_curated_decision() {
        let ws = TestWorkspace::new("promote-success");
        let root = ws.root();
        seed_promotable_run(root, "promote-ok");

        handle_council_promote("promote-ok", "layers", None, false, true).unwrap();

        let records =
            load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["entity"], "decision");
        assert_eq!(records[0]["source"], "council-promotion-v1");
    }

    #[test]
    fn promote_dry_run_does_not_write() {
        let ws = TestWorkspace::new("promote-dry");
        let root = ws.root();
        seed_promotable_run(root, "promote-dry");

        handle_council_promote("promote-dry", "layers", None, true, true).unwrap();

        let records =
            load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn promote_rejects_duplicate() {
        let ws = TestWorkspace::new("promote-dup");
        let root = ws.root();
        seed_promotable_run(root, "promote-dup");

        handle_council_promote("promote-dup", "layers", None, false, true).unwrap();
        let result = handle_council_promote("promote-dup", "layers", None, false, true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already promoted"));
    }
}
