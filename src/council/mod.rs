mod artifacts;
mod convergence;
mod stage;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{memoryport_dir, workspace_root};
use crate::types::{CouncilConvergenceRecord, CouncilRunRecord, ImpactSummary};
use crate::util::iso_now;

use artifacts::{
    append_trace_record, build_run_id, degraded_reasons, initial_stage_record, persist_run_state,
    validate_run_artifacts,
};
use convergence::{build_convergence_record, build_failure_convergence_record};
use stage::{StageOutcome, StageSpec, execute_stage};

pub struct CouncilRunRequest {
    pub task: String,
    pub route: String,
    pub context_text: String,
    pub context_json: serde_json::Value,
    pub graph_context: Option<ImpactSummary>,
    pub targets: Vec<String>,
    pub gemini_cmd: String,
    pub claude_cmd: String,
    pub codex_cmd: String,
    pub retry_limit: u32,
    pub timeout_secs: u64,
    pub artifacts_dir: Option<PathBuf>,
    pub trace_path_override: Option<PathBuf>,
}

pub fn execute_council_run(request: CouncilRunRequest) -> Result<CouncilRunRecord> {
    let created_at = iso_now();
    let run_id = build_run_id(&request.task, &created_at);
    let graph_context = request.graph_context.clone();
    let artifacts_dir = request
        .artifacts_dir
        .unwrap_or_else(|| memoryport_dir().join("council-runs").join(&run_id));
    fs::create_dir_all(&artifacts_dir)?;

    let context_text_path = artifacts_dir.join("context.txt");
    let context_json_path = artifacts_dir.join("context.json");
    fs::write(&context_text_path, &request.context_text)?;
    fs::write(
        &context_json_path,
        serde_json::to_string_pretty(&request.context_json)?,
    )?;

    let mut run = CouncilRunRecord {
        run_id: run_id.clone(),
        task: request.task.clone(),
        status: "running".to_string(),
        status_reason: "running".to_string(),
        created_at: created_at.clone(),
        updated_at: created_at,
        workspace_root: workspace_root().display().to_string(),
        artifacts_dir: artifacts_dir.display().to_string(),
        route: request.route,
        targets: request.targets,
        graph_context: graph_context.clone(),
        context_text_path: context_text_path.display().to_string(),
        context_json_path: context_json_path.display().to_string(),
        retry_limit: request.retry_limit,
        timeout_secs: request.timeout_secs,
        degraded_reasons: degraded_reasons(&request.context_json),
        artifact_errors: vec![],
        stages: vec![
            initial_stage_record(
                "gemini",
                "Gemini",
                "Generate options before convergence.",
                &artifacts_dir,
            ),
            initial_stage_record(
                "claude",
                "Claude",
                "Critique Gemini's draft and surface risks.",
                &artifacts_dir,
            ),
            initial_stage_record(
                "codex",
                "Codex",
                "Converge on the smallest reliable executable outcome.",
                &artifacts_dir,
            ),
        ],
        convergence: None,
    };
    persist_run_state(&artifacts_dir, &run)?;

    let specs = [
        StageSpec {
            stage: "gemini",
            model: "Gemini",
            role: "Generate options before convergence.",
            command: &request.gemini_cmd,
        },
        StageSpec {
            stage: "claude",
            model: "Claude",
            role: "Critique Gemini's draft and surface risks.",
            command: &request.claude_cmd,
        },
        StageSpec {
            stage: "codex",
            model: "Codex",
            role: "Converge on the smallest reliable executable outcome.",
            command: &request.codex_cmd,
        },
    ];

    let mut prior_outputs = Vec::new();
    for (index, spec) in specs.iter().enumerate() {
        let prompt = build_stage_prompt(
            spec.stage,
            spec.model,
            spec.role,
            &request.task,
            &request.context_text,
            &graph_context,
            &prior_outputs,
        );
        let output = execute_stage(
            &artifacts_dir,
            &mut run,
            index,
            spec,
            &prompt,
            request.retry_limit,
            request.timeout_secs,
        )?;
        match output {
            StageOutcome::Succeeded(output) => {
                prior_outputs.push((spec.stage.to_string(), output));
            }
            StageOutcome::Failed { reason } => {
                run.status = "failed".to_string();
                run.status_reason = reason;
                run.updated_at = iso_now();
                break;
            }
        }
    }

    let convergence = if run.status == "failed" {
        build_failure_convergence_record(&artifacts_dir, &run)?
    } else {
        let final_output = prior_outputs
            .last()
            .map(|(_, output)| output.as_str())
            .unwrap_or_default();
        let final_output_path = run
            .stages
            .last()
            .map(|stage| stage.output_path.clone())
            .unwrap_or_default();
        build_convergence_record(&artifacts_dir, final_output, &final_output_path)?
    };

    if run.status != "failed" {
        if convergence.status == "converged" {
            run.status = "completed".to_string();
            run.status_reason = "converged".to_string();
        } else {
            run.status = "incomplete".to_string();
            run.status_reason = convergence.reason.clone();
        }
    }

    run.updated_at = iso_now();
    run.convergence = Some(convergence.clone());
    run.artifact_errors = validate_run_artifacts(&artifacts_dir, &run);
    if !run.artifact_errors.is_empty() {
        run.status = "failed".to_string();
        run.status_reason = "artifact_validation_failed".to_string();
        if let Some(item) = run.convergence.as_mut() {
            item.status = "not_converged".to_string();
            item.reason = "artifact_validation_failed".to_string();
        }
    }
    persist_run_state(&artifacts_dir, &run)?;
    let trace_convergence = run.convergence.as_ref().unwrap_or(&convergence);
    append_trace_record(
        &run,
        trace_convergence,
        request.trace_path_override.as_deref(),
    )?;

    Ok(run)
}

pub fn default_run_artifacts_dir(run_id: &str) -> PathBuf {
    memoryport_dir().join("council-runs").join(run_id)
}

pub fn load_council_run_record(
    run_id: &str,
    artifacts_dir_override: Option<&Path>,
) -> Result<CouncilRunRecord> {
    let artifacts_dir = artifacts_dir_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_run_artifacts_dir(run_id));
    let run_path = artifacts_dir.join("run.json");
    let run = fs::read_to_string(&run_path)
        .with_context(|| format!("failed to read {}", run_path.display()))?;
    let record = serde_json::from_str::<CouncilRunRecord>(&run)
        .with_context(|| format!("failed to parse {}", run_path.display()))?;
    if record.run_id != run_id {
        anyhow::bail!(
            "run id mismatch: requested '{}' but artifact contains '{}'",
            run_id,
            record.run_id
        );
    }
    Ok(record)
}

pub fn load_council_convergence_record(
    run: &CouncilRunRecord,
    artifacts_dir_override: Option<&Path>,
) -> Result<CouncilConvergenceRecord> {
    let artifacts_dir = artifacts_dir_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(&run.artifacts_dir));
    let convergence_path = artifacts_dir.join("convergence.json");
    let convergence = fs::read_to_string(&convergence_path)
        .with_context(|| format!("failed to read {}", convergence_path.display()))?;
    serde_json::from_str::<CouncilConvergenceRecord>(&convergence)
        .with_context(|| format!("failed to parse {}", convergence_path.display()))
}

fn build_stage_prompt(
    stage: &str,
    model: &str,
    role: &str,
    task: &str,
    context_text: &str,
    graph_context: &Option<ImpactSummary>,
    prior_outputs: &[(String, String)],
) -> String {
    let mut prompt = vec![
        format!("You are {model} in the Layers council."),
        format!("Stage: {stage}"),
        format!("Role: {role}"),
        "Workflow order is fixed: Gemini -> Claude -> Codex.".to_string(),
        "Stay local-first and grounded in the retrieved context below.".to_string(),
        String::new(),
        "Task:".to_string(),
        task.to_string(),
        String::new(),
        "Retrieved Layers context:".to_string(),
        context_text.to_string(),
    ];

    if let Some(graph_context) = graph_context {
        prompt.push(String::new());
        prompt.push("GitNexus workflow context:".to_string());
        prompt
            .push(serde_json::to_string_pretty(graph_context).unwrap_or_else(|_| "{}".to_string()));
    }

    if !prior_outputs.is_empty() {
        prompt.push(String::new());
        prompt.push("Earlier council stages:".to_string());
        for (name, output) in prior_outputs {
            prompt.push(format!("## {}", name.to_uppercase()));
            prompt.push(output.to_string());
        }
    }

    prompt.push(String::new());
    prompt.push("Response contract:".to_string());
    match stage {
        "gemini" => {
            prompt.push("## Options".to_string());
            prompt.push("- Provide 2 or 3 viable approaches.".to_string());
            prompt.push("## Key Evidence".to_string());
            prompt.push("- Cite the strongest evidence from Layers context.".to_string());
            prompt.push("## Open Questions".to_string());
            prompt.push("- List what still needs critique.".to_string());
        }
        "claude" => {
            prompt.push("## Critique".to_string());
            prompt.push("- Challenge weak assumptions in Gemini's output.".to_string());
            prompt.push("## Risks".to_string());
            prompt.push("- Name likely failure modes.".to_string());
            prompt.push("## Best Surviving Direction".to_string());
            prompt.push("- Keep only the strongest path forward.".to_string());
        }
        "codex" => {
            prompt.push("## Decision".to_string());
            prompt.push("- State the smallest reliable path.".to_string());
            prompt.push("## Why".to_string());
            prompt.push("- Tie the decision to memory and graph evidence.".to_string());
            prompt.push("## Risks".to_string());
            prompt.push("- Keep unresolved items explicit.".to_string());
            prompt.push("## Next Steps".to_string());
            prompt.push("- Make the next actions executable.".to_string());
            prompt.push("Convergence: converged".to_string());
        }
        _ => {}
    }

    prompt.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::workspace_guard;
    use serde_json::json;

    fn temp_artifact_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("layers-{}-{}", name, iso_now().replace(':', "")));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn council_run_executes_fixed_stage_order_and_persists_artifacts() {
        let _guard = workspace_guard();
        let artifacts_dir = temp_artifact_dir("council-success");
        let request = CouncilRunRequest {
            task: "Ship the smallest reliable council flow".to_string(),
            route: "both".to_string(),
            context_text: "Route: both\nEvidence: local memory and graph".to_string(),
            context_json: json!({"route": "both"}),
            graph_context: None,
            targets: vec!["handle_remember".to_string()],
            gemini_cmd: "printf '## Options\n- option a keeps the council small and durable\n## Key Evidence\n- local memory and graph support this path\n## Open Questions\n- confirm the artifact contract is strict enough\n'".to_string(),
            claude_cmd: "printf '## Critique\n- option a is acceptable if terminal reasons stay explicit\n## Risks\n- stage output could still be too weak without hard gates\n## Best Surviving Direction\n- keep option a and harden the execution contract\n'".to_string(),
            codex_cmd: "printf '## Decision\n- implement option a\n## Why\n- grounded\n## Risks\n- residual risk\n## Next Steps\n- do the work\nConvergence: converged\n'".to_string(),
            retry_limit: 1,
            timeout_secs: 2,
            artifacts_dir: Some(artifacts_dir.clone()),
            trace_path_override: Some(artifacts_dir.join("council-traces.jsonl")),
        };

        let run = execute_council_run(request).unwrap();

        assert_eq!(run.status, "completed");
        assert_eq!(run.status_reason, "converged");
        assert_eq!(run.stages.len(), 3);
        assert!(artifacts_dir.join("run.json").exists());
        assert!(artifacts_dir.join("convergence.json").exists());
        assert_eq!(run.stages[0].model, "Gemini");
        assert_eq!(run.stages[1].model, "Claude");
        assert_eq!(run.stages[2].model, "Codex");
        assert_eq!(run.stages[2].status, "succeeded");
        assert!(
            run.convergence
                .as_ref()
                .map(|item| item.status.as_str())
                .unwrap_or_default()
                == "converged"
        );
        assert!(run.artifact_errors.is_empty());
    }

    #[test]
    fn council_run_retries_failed_stage_once() {
        let _guard = workspace_guard();
        let artifacts_dir = temp_artifact_dir("council-retry");
        let counter = artifacts_dir.join("counter.txt");
        let counter_text = counter.display().to_string();
        let retry_cmd = format!(
            "count=0; [ -f '{0}' ] && count=$(cat '{0}'); count=$((count+1)); echo $count > '{0}'; if [ \"$count\" -eq 1 ]; then echo 'first failure' >&2; exit 1; fi; printf '## Decision\\n- retry worked with a real contract\\n## Why\\n- the second attempt reused the same grounded task\\n## Risks\\n- minor residual risk\\n## Next Steps\\n- keep the final artifacts\\nConvergence: converged\\n'",
            counter_text
        );
        let request = CouncilRunRequest {
            task: "Retry on transient stage failure".to_string(),
            route: "memory_only".to_string(),
            context_text: "Route: memory_only".to_string(),
            context_json: json!({"route": "memory_only"}),
            graph_context: None,
            targets: vec![],
            gemini_cmd: "printf '## Options\n- keep retry support in the stage runner\n## Key Evidence\n- the first attempt can fail without losing artifacts\n## Open Questions\n- confirm retries remain bounded\n'".to_string(),
            claude_cmd: "printf '## Critique\n- retries are acceptable when failure evidence is preserved\n## Risks\n- silent stalls would still be dangerous\n## Best Surviving Direction\n- keep retries but fail honestly after exhaustion\n'".to_string(),
            codex_cmd: retry_cmd,
            retry_limit: 2,
            timeout_secs: 2,
            artifacts_dir: Some(artifacts_dir),
            trace_path_override: Some(counter.parent().unwrap().join("council-traces.jsonl")),
        };

        let run = execute_council_run(request).unwrap();
        let attempts = &run.stages[2].attempts;
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].status, "failed");
        assert_eq!(attempts[1].status, "succeeded");
    }

    #[test]
    fn council_run_marks_short_output_as_stall_and_stops() {
        let _guard = workspace_guard();
        let artifacts_dir = temp_artifact_dir("council-stall");
        let request = CouncilRunRequest {
            task: "Detect stage stalls honestly".to_string(),
            route: "memory_only".to_string(),
            context_text: "Route: memory_only".to_string(),
            context_json: json!({"route": "memory_only"}),
            graph_context: None,
            targets: vec![],
            gemini_cmd: "printf 'ok\n'".to_string(),
            claude_cmd: "printf '## Critique\n- should never run\n'".to_string(),
            codex_cmd: "printf '## Decision\n- should never run\n'".to_string(),
            retry_limit: 1,
            timeout_secs: 2,
            artifacts_dir: Some(artifacts_dir),
            trace_path_override: None,
        };

        let run = execute_council_run(request).unwrap();
        assert_eq!(run.status, "failed");
        assert_eq!(run.status_reason, "retries_exhausted");
        assert_eq!(run.stages[0].attempts[0].status, "stalled");
        assert_eq!(run.stages[1].status, "pending");
    }

    #[test]
    fn council_run_records_timeout_reason() {
        let _guard = workspace_guard();
        let artifacts_dir = temp_artifact_dir("council-timeout");
        let request = CouncilRunRequest {
            task: "Timeouts must be explicit".to_string(),
            route: "graph_only".to_string(),
            context_text: "Route: graph_only".to_string(),
            context_json: json!({"route": "graph_only"}),
            graph_context: None,
            targets: vec![],
            gemini_cmd: "sleep 2".to_string(),
            claude_cmd: "printf '## Critique\n- should never run\n'".to_string(),
            codex_cmd: "printf '## Decision\n- should never run\n'".to_string(),
            retry_limit: 1,
            timeout_secs: 1,
            artifacts_dir: Some(artifacts_dir),
            trace_path_override: None,
        };

        let run = execute_council_run(request).unwrap();
        assert_eq!(run.status, "failed");
        assert_eq!(run.status_reason, "stage_timed_out");
        assert_eq!(run.stages[0].attempts[0].status, "timed_out");
        assert_eq!(
            run.convergence.as_ref().map(|item| item.reason.as_str()),
            Some("stage_timed_out")
        );
    }

    #[test]
    fn council_run_is_incomplete_when_codex_does_not_meet_contract() {
        let _guard = workspace_guard();
        let artifacts_dir = temp_artifact_dir("council-incomplete");
        let request = CouncilRunRequest {
            task: "Non converged output should stay honest".to_string(),
            route: "both".to_string(),
            context_text: "Route: both".to_string(),
            context_json: json!({"route": "both"}),
            graph_context: None,
            targets: vec![],
            gemini_cmd: "printf '## Options\n- keep the council narrow and grounded\n## Key Evidence\n- the architecture is already in place\n## Open Questions\n- verify the Codex contract\n'".to_string(),
            claude_cmd: "printf '## Critique\n- the final stage needs a real convergence contract\n## Risks\n- formatting luck could mark convergence incorrectly\n## Best Surviving Direction\n- require decision plus next steps\n'".to_string(),
            codex_cmd: "printf '## Decision\n- useful but incomplete decision text\n## Why\n- some reasoning exists\n## Risks\n- next steps are missing\nConvergence: converged\n'".to_string(),
            retry_limit: 1,
            timeout_secs: 2,
            artifacts_dir: Some(artifacts_dir),
            trace_path_override: None,
        };

        let run = execute_council_run(request).unwrap();
        assert_eq!(run.status, "incomplete");
        assert_eq!(run.status_reason, "missing_required_sections");
        assert_eq!(
            run.convergence.as_ref().map(|item| item.status.as_str()),
            Some("not_converged")
        );
    }
}
