use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::config::{audit_path, canonical_curated_memory_path, memoryport_dir, workspace_root};
use crate::council::{
    CouncilRunRequest, execute_council_run, load_council_convergence_record,
    load_council_run_record,
};
use crate::graph::{gitnexus_index_version, gitnexus_indexed, impact_summary, query_graph};
use crate::memory::search_memory;
use crate::projects::{
    append_curated_record, canonical_project_slug, create_project, create_task,
    import_curated_memory, list_projects, load_curated_memory, project_summary_line,
    require_project_exists, task_project_map, task_summary_line, validate_record_shapes,
};
use crate::routing::route_query;
use crate::synthesis::build_context;
use crate::types::{
    Decision, GraphContext, ImplementationContext, ProjectRecord, ProjectRecordPayload,
    ReviewContext,
};
use crate::util::{append_jsonl, compact, iso_now, load_jsonl, which};

pub fn run_query(query: &str, emit_audit: bool) -> Result<Value> {
    let started = Instant::now();
    let decision = route_query(query);
    let (memory_hits, memory_issue) = if decision.route == "memory_only" || decision.route == "both"
    {
        search_memory(query, 3)?
    } else {
        (vec![], None)
    };
    let (graph_hits, graph_issue) = if decision.route == "graph_only" || decision.route == "both" {
        query_graph(query, 5)?
    } else {
        (vec![], None)
    };
    let mut payload = build_context(
        query,
        &decision,
        &memory_hits,
        memory_issue.as_deref(),
        &graph_hits,
        graph_issue.as_deref(),
    )?;
    let duration_ms = started.elapsed().as_millis() as u64;
    let audit = json!({
        "timestamp": iso_now(),
        "query": query,
        "route": decision.route,
        "confidence": decision.confidence,
        "scores": decision.scores,
        "rationale": decision.rationale,
        "memory_results": memory_hits.len(),
        "memory_issue": memory_issue,
        "graph_results": graph_hits.len(),
        "graph_issue": graph_issue,
        "duration_ms": duration_ms,
    });
    payload["audit"] = audit.clone();
    if emit_audit {
        append_jsonl(&audit_path(), &audit)?;
    }
    Ok(payload)
}

pub fn handle_query(query: &str, json_out: bool, no_audit: bool) -> Result<()> {
    let payload = run_query(query, !no_audit)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if let Some(text) = payload.get("context_text").and_then(|v| v.as_str()) {
        println!("{}", text);
    }
    Ok(())
}

fn existing_embeddings_requested() -> bool {
    let meta = workspace_root().join(".gitnexus").join("meta.json");
    if !meta.exists() {
        return false;
    }
    let Ok(data) = fs::read_to_string(meta) else {
        return true;
    };
    data.to_lowercase().contains("embedding")
}

pub fn handle_refresh() -> Result<()> {
    if which("gitnexus").is_none() {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"ok": false, "error": "gitnexus not installed"}))?
        );
        anyhow::bail!("gitnexus not installed");
    }
    let use_embeddings =
        workspace_root().join(".gitnexus").exists() || existing_embeddings_requested();
    let mut cmd = Command::new("gitnexus");
    cmd.current_dir(workspace_root()).arg("analyze");
    if use_embeddings {
        cmd.arg("--embeddings");
    }
    cmd.arg(workspace_root());
    let output = cmd.output()?;
    let mut command_args = vec!["gitnexus".to_string(), "analyze".to_string()];
    if use_embeddings {
        command_args.push("--embeddings".to_string());
    }
    command_args.push(workspace_root().display().to_string());
    let payload = json!({
        "ok": output.status.success(),
        "command": command_args,
        "stdout": compact(&String::from_utf8_lossy(&output.stdout), 600),
        "stderr": compact(&String::from_utf8_lossy(&output.stderr), 600),
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

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
            let plan_markdown = fs::read_to_string(file)?;
            let target_symbols = parse_targets(targets.as_deref());
            let graph_context = build_graph_context(&target_symbols)?;
            json!({
                "timestamp": iso_now(),
                "task_type": task_type.unwrap_or_else(|| "architecture".to_string()),
                "task": task,
                "plan_markdown": plan_markdown,
                "artifacts_dir": artifacts_dir,
                "targets": target_symbols,
                "metadata": {
                    "graph_context": graph_context,
                },
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
        _ => anyhow::bail!("unsupported kind: {}", kind),
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

fn parse_targets(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect()
}

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
    let payload = run_query(task, false)?;
    let target_symbols = parse_targets(targets.as_deref());
    let graph_context = build_graph_context(&target_symbols)?;
    let run = execute_council_run(CouncilRunRequest {
        task: task.to_string(),
        route: payload
            .get("route")
            .and_then(|value| value.as_str())
            .unwrap_or("neither")
            .to_string(),
        context_text: payload
            .get("context_text")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        context_json: payload,
        graph_context,
        targets: target_symbols,
        gemini_cmd,
        claude_cmd,
        codex_cmd,
        retry_limit,
        timeout_secs,
        artifacts_dir: artifacts_dir.map(std::path::PathBuf::from),
        trace_path_override: None,
    })?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&run)?);
    } else {
        let convergence = run
            .convergence
            .as_ref()
            .map(|item| format!("{} ({}): {}", item.status, item.reason, item.summary))
            .unwrap_or_else(|| "no convergence record".to_string());
        println!(
            "Council run {} {} ({})\nArtifacts: {}\n{}",
            run.run_id, run.status, run.status_reason, run.artifacts_dir, convergence
        );
        if !run.degraded_reasons.is_empty() {
            println!("Degraded: {}", run.degraded_reasons.join(" | "));
        }
        if !run.artifact_errors.is_empty() {
            println!("Artifact errors: {}", run.artifact_errors.join(" | "));
        }
    }
    Ok(())
}

pub fn handle_council_promote(
    run_id: &str,
    project: &str,
    artifacts_dir: Option<String>,
    dry_run: bool,
    json_out: bool,
) -> Result<()> {
    require_project_exists(project)?;
    let project_slug = canonical_project_slug(project)?;
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
    if load_curated_memory()?
        .iter()
        .any(|existing| existing.id == record.id)
    {
        anyhow::bail!(
            "run '{}' was already promoted into canonical curated memory as '{}'",
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
        "source_artifact": artifacts_dir_path
            .as_deref()
            .map(|path| path.join("convergence.json"))
            .unwrap_or_else(|| PathBuf::from(&run.artifacts_dir).join("convergence.json")),
        "canonical_path": canonical_curated_memory_path(),
        "record": record,
    });

    if !dry_run {
        append_curated_record(&record)?;
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

fn build_graph_context(target_symbols: &[String]) -> Result<Option<GraphContext>> {
    if target_symbols.is_empty() {
        return Ok(None);
    }
    let impact = impact_summary(target_symbols)?;
    let affected_flows = impact
        .as_ref()
        .map(|item| item.affected_processes.clone())
        .unwrap_or_default();
    Ok(Some(GraphContext {
        gitnexus_index_version: gitnexus_index_version()?.unwrap_or_default(),
        impact_summary: impact,
        implementation_context: Some(ImplementationContext {
            target_symbols: target_symbols.to_vec(),
            changed_files: vec![],
            affected_flows: affected_flows.clone(),
        }),
        review_context: Some(ReviewContext {
            before_scope: target_symbols.to_vec(),
            after_scope: affected_flows,
            drift_symbols: vec![],
        }),
    }))
}

fn council_promotion_record(
    run: &crate::types::CouncilRunRecord,
    convergence: &crate::types::CouncilConvergenceRecord,
    project: &str,
) -> Result<ProjectRecord> {
    let decision_text = convergence.decision.trim();
    if decision_text.is_empty() {
        anyhow::bail!(
            "run '{}' convergence artifact does not contain a structured decision",
            run.run_id
        );
    }

    let promoted_at = iso_now();
    let slug = deterministic_council_decision_slug(&run.run_id);
    let source_artifact = Path::new(&run.artifacts_dir).join("convergence.json");
    let rationale = if convergence.why.is_empty() {
        format!("Promoted from converged council run {}.", run.run_id)
    } else {
        convergence.why.join(" ")
    };

    Ok(ProjectRecord {
        id: format!("cm_decision_council_{}", slug),
        entity: "decision".to_string(),
        project: project.trim().to_string(),
        task: None,
        created_at: promoted_at.clone(),
        source: "council-promotion-v1".to_string(),
        tags: vec!["council".to_string(), "promoted".to_string()],
        archived: false,
        metadata: Some(json!({
            "promotion": {
                "version": "v1",
                "promoted_at": promoted_at,
                "run_id": run.run_id,
                "artifacts_dir": run.artifacts_dir,
                "source_artifact": source_artifact,
                "convergence_output_path": convergence.output_path,
                "route": run.route,
                "targets": run.targets,
                "status": run.status,
                "status_reason": run.status_reason,
                "convergence_status": convergence.status,
                "convergence_reason": convergence.reason,
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

fn deterministic_council_decision_slug(run_id: &str) -> String {
    run_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn council_command(stage: &str, explicit: Option<String>) -> Result<String> {
    if let Some(command) = explicit {
        if !command.trim().is_empty() {
            return Ok(command);
        }
    }
    let env_key = match stage {
        "gemini" => "LAYERS_COUNCIL_GEMINI_CMD",
        "claude" => "LAYERS_COUNCIL_CLAUDE_CMD",
        "codex" => "LAYERS_COUNCIL_CODEX_CMD",
        _ => anyhow::bail!("unsupported council stage {}", stage),
    };
    std::env::var(env_key).with_context(|| {
        format!(
            "{} not set; pass --{}-cmd or configure the environment variable",
            env_key, stage
        )
    })
}

pub fn handle_validate() -> Result<()> {
    let cases = vec![
        (
            "What did we already decide about JIT experts and why did we stop the last round?",
            "memory_only",
        ),
        (
            "What files and modules would this refactor touch in the repo?",
            "graph_only",
        ),
        (
            "Implement the approved Layers design in the current repo layout.",
            "both",
        ),
        ("Rename this variable in the snippet below.", "neither"),
    ];
    let mut routing_ok = true;
    let mut routing_cases = Vec::new();
    for (query, expected) in cases {
        let decision = route_query(query);
        let pass = decision.route == expected;
        routing_ok &= pass;
        routing_cases.push(json!({
            "query": query,
            "expected": expected,
            "actual": decision.route,
            "ok": pass,
            "scores": decision.scores,
        }));
    }

    let audit_before = load_jsonl(&audit_path())?.len();
    let end_to_end_query =
        "What did we decide last time about Layers and how should it relate to GitNexus?";
    let end_to_end = run_query(end_to_end_query, true)?;
    let audit_after = load_jsonl(&audit_path())?.len();
    let audit_smoke = audit_after == audit_before + 1;
    let end_to_end_memory_results = end_to_end
        .get("evidence")
        .and_then(|v| v.get("memory"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0);
    let architecture_summary_items = end_to_end
        .get("architecture_summary")
        .and_then(|v| v.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0);
    let typed_memory_brief_ok = end_to_end
        .get("memory_brief")
        .and_then(|v| v.as_object())
        .is_some();
    let memory_issue = end_to_end
        .get("audit")
        .and_then(|v| v.get("memory_issue"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let memory_provider_ok = end_to_end_memory_results > 0;

    let (indexed, status, repo) = gitnexus_indexed()?;
    let (graph_workflow_results, graph_workflow_issue) = if indexed {
        let (hits, issue) = query_graph(
            "layers architecture routing memory gitnexus validation workflow",
            3,
        )?;
        (hits.len(), issue)
    } else {
        (0, Some(status.clone()))
    };
    let project_records_ok = validate_record_shapes().is_ok();
    let council_commands_configured = [
        "LAYERS_COUNCIL_GEMINI_CMD",
        "LAYERS_COUNCIL_CLAUDE_CMD",
        "LAYERS_COUNCIL_CODEX_CMD",
    ]
    .iter()
    .all(|key| {
        std::env::var(key)
            .ok()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    });
    let repo_prepared = indexed && memory_provider_ok;
    let contract_ok = routing_ok && audit_smoke && indexed && project_records_ok;

    let payload = json!({
        "ok": contract_ok,
        "routing": {
            "ok": routing_ok,
            "cases": routing_cases,
        },
        "memory_provider": {
            "ok": memory_provider_ok,
            "ready_for_replacement": memory_provider_ok,
            "issue": memory_issue,
            "typed_memory_brief_ok": typed_memory_brief_ok,
        },
        "project_records": {
            "ok": project_records_ok,
        },
        "graph_provider": {
            "indexed": indexed,
            "status": status,
            "repo": repo,
        },
        "graph_workflow": {
            "results": graph_workflow_results,
            "issue": graph_workflow_issue,
        },
        "council_workflow": {
            "roles": ["Gemini", "Claude", "Codex"],
            "order": "Gemini -> Claude -> Codex",
            "commands_configured": council_commands_configured,
        },
        "repo_prepared": repo_prepared,
        "end_to_end_query": {
            "query": end_to_end_query,
            "route": end_to_end.get("route"),
            "memory_results": end_to_end_memory_results,
            "architecture_summary_items": architecture_summary_items,
            "audit_incremented": audit_smoke,
            "audit_before": audit_before,
            "audit_after": audit_after,
        },
        "replacement_ready": contract_ok && repo_prepared,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub fn handle_curated_import(file: &str) -> Result<()> {
    let path = std::path::Path::new(file);
    let (imported, skipped) = import_curated_memory(path)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "file": path,
            "canonical_path": canonical_curated_memory_path(),
            "imported": imported,
            "skipped": skipped,
        }))?
    );
    Ok(())
}

pub fn handle_project_create(
    slug: &str,
    title: &str,
    summary: Option<String>,
    status: Option<String>,
) -> Result<()> {
    let record = create_project(slug, title, summary.as_deref(), status.as_deref())?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(())
}

pub fn handle_project_list(json_out: bool) -> Result<()> {
    let projects = list_projects()?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&projects)?);
        return Ok(());
    }
    if projects.is_empty() {
        println!("No projects found.");
        return Ok(());
    }
    for project in projects {
        println!("{}", project_summary_line(&project));
    }
    Ok(())
}

pub fn handle_task_create(
    project: &str,
    slug: &str,
    title: &str,
    summary: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    acceptance: Option<String>,
) -> Result<()> {
    require_project_exists(project)?;
    let record = create_task(
        project,
        slug,
        title,
        summary.as_deref(),
        status.as_deref(),
        priority.as_deref(),
        acceptance.as_deref(),
    )?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(())
}

pub fn handle_task_list(
    project: Option<String>,
    status: Option<String>,
    json_out: bool,
) -> Result<()> {
    let tasks = task_project_map(project.as_deref(), status.as_deref())?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&tasks)?);
        return Ok(());
    }
    if let Some(ref project_slug) = project {
        require_project_exists(project_slug)?;
    }
    if tasks.is_empty() {
        println!("No tasks found.");
        return Ok(());
    }
    for (project_slug, task) in tasks {
        println!("{}", task_summary_line(&project_slug, &task));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::handle_council_promote;
    use crate::test_support::TestWorkspace;
    use crate::types::{Project, ProjectRecord, ProjectRecordPayload};
    use crate::util::{append_jsonl, load_jsonl};
    use serde_json::json;
    use std::fs;
    use std::path::Path;

    fn seed_project(root: &Path, slug: &str) {
        let record = ProjectRecord {
            id: format!("pm_project_{}", slug),
            entity: "project".to_string(),
            project: slug.to_string(),
            task: None,
            created_at: "2026-04-01T00:00:00Z".to_string(),
            source: "manual".to_string(),
            tags: vec![],
            archived: false,
            metadata: None,
            payload: ProjectRecordPayload::Project(Project {
                slug: slug.to_string(),
                title: "Layers".to_string(),
                summary: "Local-first context router".to_string(),
                status: "active".to_string(),
            }),
        };
        append_jsonl(
            &root.join("memoryport").join("curated-memory.jsonl"),
            &serde_json::to_value(record).unwrap(),
        )
        .unwrap();
    }

    fn write_run_artifacts(root: &Path, run_id: &str, run_status: &str, convergence_status: &str) {
        let artifacts_dir = root.join("memoryport").join("council-runs").join(run_id);
        fs::create_dir_all(&artifacts_dir).unwrap();
        fs::write(artifacts_dir.join("context.txt"), "Route: both").unwrap();
        fs::write(artifacts_dir.join("context.json"), "{}").unwrap();

        let convergence_output_path = artifacts_dir.join("codex-attempt-1.stdout.txt");
        fs::write(
            &convergence_output_path,
            "## Decision\n- promote the council outcome\n## Why\n- structured convergence is canonical\n## Risks\n- minimal risk\n## Next Steps\n- append one decision\nConvergence: converged\n",
        )
        .unwrap();

        let run = json!({
            "run_id": run_id,
            "task": "Promote the council outcome",
            "status": run_status,
            "status_reason": if run_status == "completed" { "converged" } else { "missing_required_sections" },
            "created_at": "2026-04-01T00:00:00Z",
            "updated_at": "2026-04-01T00:01:00Z",
            "workspace_root": root,
            "artifacts_dir": artifacts_dir,
            "route": "both",
            "targets": ["handle_council_promote"],
            "context_text_path": artifacts_dir.join("context.txt"),
            "context_json_path": artifacts_dir.join("context.json"),
            "retry_limit": 1,
            "timeout_secs": 30,
            "degraded_reasons": [],
            "artifact_errors": [],
            "stages": [],
            "convergence": {
                "status": convergence_status,
                "reason": if convergence_status == "converged" { "converged" } else { "missing_required_sections" },
                "decision": if convergence_status == "converged" { "promote the council outcome" } else { "" },
                "summary": "promote the council outcome",
                "why": ["structured convergence is canonical"],
                "unresolved": [],
                "next_steps": ["append one decision"],
                "missing_sections": if convergence_status == "converged" { json!([]) } else { json!(["next_steps"]) },
                "output_path": convergence_output_path,
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
    fn council_promote_appends_one_curated_decision_record() {
        let workspace = TestWorkspace::new("commands-promote-success");
        let root = workspace.root();
        seed_project(&root, "layers");
        write_run_artifacts(
            &root,
            "council-20260401-promote-success",
            "completed",
            "converged",
        );

        handle_council_promote(
            "council-20260401-promote-success",
            "layers",
            None,
            false,
            true,
        )
        .unwrap();

        let records = load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
        assert_eq!(records.len(), 2);
        let promoted = records.last().unwrap();
        assert_eq!(promoted["entity"], "decision");
        assert_eq!(promoted["source"], "council-promotion-v1");
        assert_eq!(
            promoted["metadata"]["promotion"]["run_id"],
            "council-20260401-promote-success"
        );
        assert_eq!(
            promoted["metadata"]["promotion"]["source_artifact"],
            root.join("memoryport")
                .join("council-runs")
                .join("council-20260401-promote-success")
                .join("convergence.json")
                .display()
                .to_string()
        );
    }

    #[test]
    fn council_promote_dry_run_does_not_write() {
        let workspace = TestWorkspace::new("commands-promote-dry-run");
        let root = workspace.root();
        seed_project(&root, "layers");
        write_run_artifacts(
            &root,
            "council-20260401-promote-dry-run",
            "completed",
            "converged",
        );

        handle_council_promote(
            "council-20260401-promote-dry-run",
            "layers",
            None,
            true,
            true,
        )
        .unwrap();

        let records = load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn council_promote_rejects_duplicate_promotion() {
        let workspace = TestWorkspace::new("commands-promote-duplicate");
        let root = workspace.root();
        seed_project(&root, "layers");
        write_run_artifacts(
            &root,
            "council-20260401-promote-duplicate",
            "completed",
            "converged",
        );

        handle_council_promote(
            "council-20260401-promote-duplicate",
            "layers",
            None,
            false,
            true,
        )
        .unwrap();
        let err = handle_council_promote(
            "council-20260401-promote-duplicate",
            "layers",
            None,
            false,
            true,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("already promoted into canonical curated memory")
        );
    }

    #[test]
    fn council_promote_rejects_non_promotable_runs() {
        let workspace = TestWorkspace::new("commands-promote-reject");
        let root = workspace.root();
        seed_project(&root, "layers");
        write_run_artifacts(
            &root,
            "council-20260401-promote-incomplete",
            "incomplete",
            "not_converged",
        );
        write_run_artifacts(
            &root,
            "council-20260401-promote-failed",
            "failed",
            "not_converged",
        );
        write_run_artifacts(
            &root,
            "council-20260401-promote-not-converged",
            "completed",
            "not_converged",
        );

        let incomplete = handle_council_promote(
            "council-20260401-promote-incomplete",
            "layers",
            None,
            false,
            true,
        )
        .unwrap_err();
        assert!(incomplete.to_string().contains("not promotable"));

        let failed = handle_council_promote(
            "council-20260401-promote-failed",
            "layers",
            None,
            false,
            true,
        )
        .unwrap_err();
        assert!(failed.to_string().contains("not promotable"));

        let not_converged = handle_council_promote(
            "council-20260401-promote-not-converged",
            "layers",
            None,
            false,
            true,
        )
        .unwrap_err();
        assert!(not_converged.to_string().contains("not converged"));
    }
}
