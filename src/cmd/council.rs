use anyhow::Result;
use serde_json::json;
use std::path::PathBuf;

use crate::cmd::query::{RetrievalMeta, build_context_payload};
use crate::config::{canonical_curated_memory_path, workspace_root};
use crate::council::{
    CouncilRunRequest, execute_council_run, load_council_convergence_record,
    load_council_run_record,
};
use crate::graph;
use crate::memory;
use crate::types::{Decision, ProjectRecord, ProjectRecordPayload};
use crate::uc;
use crate::util::{append_jsonl, compact, iso_now, load_jsonl, parse_targets, run_command, which};

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
    let graph_context = graph::impact(&target_symbols)?;

    // Build structured context payload for the council handshake
    let context_payload = build_context_payload(
        task,
        crate::router::Route::Both,
        "high",
        Vec::new(),
        Vec::new(),
        RetrievalMeta {
            memory_source: "direct".to_string(),
            memory_latency_ms: 0,
            graph_latency_ms: 0,
            fallback_reason: None,
        },
    );
    let payload_value = serde_json::to_value(&context_payload)?;

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
        context_payload: Some(payload_value),
    })?;

    if json_out {
        println!("{}", serde_json::to_string_pretty(&run)?);
    } else {
        let convergence = run
            .convergence
            .as_ref()
            .map_or_else(|| "no convergence record".to_string(), |c| format!("{} ({}): {}", c.status, c.reason, c.summary));
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

/// Gather context by calling `MemoryPort` and `GitNexus` directly.
/// No routing heuristics — just retrieve from both and concatenate.
fn gather_context(task: &str) -> Result<String> {
    let mut sections = Vec::new();

    // MemoryPort semantic retrieval via the `uc` module (timeout + threshold protected).
    // Important: the local `codex-memoryport-bridge` is an OpenAI Responses proxy,
    // not a generic MCP tool server, so Layers intentionally talks to MemoryPort
    // through `uc` and canonical files rather than assuming a raw tool surface.
    let uc_retriever = uc::UcRetriever::new(uc::UcOptions::default());
    let uc_result = uc_retriever.retrieve(task, 5);
    if uc::meets_threshold_with(&uc_result, uc_retriever.min_results()) {
        let joined = uc_result.lines.join("\n");
        if !joined.trim().is_empty() {
            sections.push(format!("## MemoryPort\n{}", joined.trim()));
        }
    }

    // JSONL memory spine using shared memory module
    let spine_records = memory::retrieve_recent(3)?;
    let spine_hits: Vec<String> = spine_records
        .iter()
        .map(|r| format!("- [{}] {}", r.source, r.text))
        .collect();
    if !spine_hits.is_empty() {
        sections.push(format!("## Memory Spine\n{}", spine_hits.join("\n")));
    }

    // GitNexus graph query (with --repo to avoid multi-repo ambiguity)
    if which("gitnexus").is_some() {
        let repo = graph::repo_name();
        let args = ["gitnexus", "query", task, "--limit", "5", "--repo", &repo];
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
        anyhow::bail!("run '{}' was already promoted as '{}'", run_id, record.id);
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
        id: format!("cm_decision_council_{slug}"),
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

pub(crate) fn council_command(stage: &str, explicit: Option<String>) -> Result<String> {
    use anyhow::Context;
    if let Some(cmd) = explicit {
        if !cmd.trim().is_empty() {
            return Ok(cmd);
        }
    }
    let env_key = match stage {
        "gemini" => "LAYERS_COUNCIL_GEMINI_CMD",
        "claude" => "LAYERS_COUNCIL_CLAUDE_CMD",
        "codex" => "LAYERS_COUNCIL_CODEX_CMD",
        _ => anyhow::bail!("unsupported council stage: {stage}"),
    };
    std::env::var(env_key).with_context(|| {
        format!(
            "{env_key} not set; pass --{stage}-cmd or set the environment variable"
        )
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
    use std::path::Path;

    fn seed_promotable_run(root: &Path, run_id: &str) {
        let artifacts_dir = root.join("memoryport").join("council-runs").join(run_id);
        fs::create_dir_all(&artifacts_dir).unwrap();

        let convergence_output = artifacts_dir.join("codex-attempt-1.stdout.txt");
        fs::write(
            &convergence_output,
            "## Decision\n- adopt this\n## Why\n- it works\n",
        )
        .unwrap();

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
    fn council_command_prefers_explicit_over_env() {
        let result = council_command("gemini", Some("echo test".to_string()));
        assert_eq!(result.unwrap(), "echo test");
    }

    #[test]
    #[allow(unsafe_code)]
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
    #[allow(unsafe_code)]
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

    #[test]
    fn promote_writes_curated_decision() {
        let ws = TestWorkspace::new("promote-success");
        let root = ws.root();
        seed_promotable_run(root, "promote-ok");

        handle_council_promote("promote-ok", "layers", None, false, true).unwrap();

        let records = load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
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

        let records = load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
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
