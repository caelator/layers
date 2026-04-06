use anyhow::Result;
use serde_json::json;
use std::fs;
use std::path::Path;

use crate::config::memoryport_dir;
use crate::types::{CouncilConvergenceRecord, CouncilRunRecord, CouncilStageRecord};
use crate::util::{append_jsonl, iso_now};

pub fn initial_stage_record(
    stage: &str,
    model: &str,
    role: &str,
    artifacts_dir: &Path,
) -> CouncilStageRecord {
    CouncilStageRecord {
        stage: stage.to_string(),
        model: model.to_string(),
        role: role.to_string(),
        status: "pending".to_string(),
        prompt_path: artifacts_dir
            .join(format!("{stage}-prompt.txt"))
            .display()
            .to_string(),
        output_path: String::new(),
        summary: String::new(),
        attempts: vec![],
    }
}

pub fn persist_run_state(artifacts_dir: &Path, run: &CouncilRunRecord) -> Result<()> {
    fs::write(
        artifacts_dir.join("run.json"),
        serde_json::to_string_pretty(run)?,
    )?;
    Ok(())
}

pub fn append_trace_record(
    run: &CouncilRunRecord,
    convergence: &CouncilConvergenceRecord,
    trace_path_override: Option<&Path>,
) -> Result<()> {
    let stage_statuses = run
        .stages
        .iter()
        .map(|stage| {
            json!({
                "stage": stage.stage,
                "model": stage.model,
                "status": stage.status,
                "output_path": stage.output_path,
            })
        })
        .collect::<Vec<_>>();
    let record = json!({
        "timestamp": iso_now(),
        "task": run.task,
        "summary": format!(
            "Council run {} {} via Gemini -> Claude -> Codex. {}",
            run.run_id, run.status, convergence.summary
        ),
        "task_type": "council",
        "artifacts_dir": run.artifacts_dir,
        "metadata": {
            "run_id": run.run_id,
            "route": run.route,
            "targets": run.targets,
            "graph_context": run.graph_context,
            "convergence": convergence,
            "stages": stage_statuses,
        }
    });
    let trace_path = trace_path_override
        .map_or_else(|| memoryport_dir().join("council-traces.jsonl"), Path::to_path_buf);
    append_jsonl(&trace_path, &record)
}

pub fn build_run_id(task: &str, created_at: &str) -> String {
    let mut slug = task
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug = slug.trim_matches('-').to_string();
    let short_slug = if slug.is_empty() {
        "task".to_string()
    } else {
        slug.chars().take(40).collect()
    };
    let stamp = created_at
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .chars()
        .take(14)
        .collect::<String>();
    format!("council-{stamp}-{short_slug}")
}

pub fn degraded_reasons(context_json: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(audit) = context_json.get("audit") {
        if let Some(issue) = audit.get("memory_issue").and_then(|value| value.as_str())
            && !issue.trim().is_empty()
        {
            out.push(format!("memory: {}", issue.trim()));
        }
        if let Some(issue) = audit.get("graph_issue").and_then(|value| value.as_str())
            && !issue.trim().is_empty()
        {
            out.push(format!("graph: {}", issue.trim()));
        }
    }
    out
}

pub fn output_quality_error(stage: &str, stdout: &str) -> Option<String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Some("stage produced empty output".to_string());
    }

    let word_count = trimmed.split_whitespace().count();
    let minimum_words = match stage {
        "codex" => 10,
        _ => 8,
    };
    if word_count < minimum_words {
        return Some(format!(
            "stage output failed quality gate: {word_count} words < {minimum_words}"
        ));
    }
    if !trimmed.contains("## ") {
        return Some("stage output failed quality gate: missing section headings".to_string());
    }
    None
}

pub fn validate_run_artifacts(artifacts_dir: &Path, run: &CouncilRunRecord) -> Vec<String> {
    let mut errors = Vec::new();
    let run_path = artifacts_dir.join("run.json");
    if !run_path.exists() {
        errors.push("missing run.json".to_string());
    }
    if !Path::new(&run.context_text_path).exists() {
        errors.push("missing context.txt artifact".to_string());
    }
    if !Path::new(&run.context_json_path).exists() {
        errors.push("missing context.json artifact".to_string());
    }
    if !artifacts_dir.join("convergence.json").exists() {
        errors.push("missing convergence.json".to_string());
    }

    for stage in &run.stages {
        let stage_reached = stage.status != "pending";
        let stage_required = run.status != "failed" || stage_reached;
        if stage_required && !Path::new(&stage.prompt_path).exists() {
            errors.push(format!("missing prompt artifact for {}", stage.stage));
        }
        for attempt in &stage.attempts {
            if !Path::new(&attempt.stdout_path).exists() {
                errors.push(format!(
                    "missing stdout artifact for {} attempt {}",
                    stage.stage, attempt.attempt
                ));
            }
            if !Path::new(&attempt.stderr_path).exists() {
                errors.push(format!(
                    "missing stderr artifact for {} attempt {}",
                    stage.stage, attempt.attempt
                ));
            }
        }
        if stage.status == "succeeded"
            && (!Path::new(&stage.output_path).exists() || stage.output_path.is_empty())
        {
            errors.push(format!("missing output artifact for {}", stage.stage));
        }
    }

    errors
}
