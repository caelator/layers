mod artifacts;
mod circuit_breaker;
mod convergence;
mod route_corrections;
mod stage;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{CONTEXT_PAYLOAD_SCHEMA_VERSION, memoryport_dir, workspace_root};
use crate::feedback::RouteId;
use crate::types::{CouncilConvergenceRecord, CouncilRunRecord, ImpactSummary};
use crate::util::iso_now;

use artifacts::{
    append_trace_record, build_run_id, degraded_reasons, initial_stage_record, persist_run_state,
    validate_run_artifacts,
};
use circuit_breaker::from_env;
use convergence::{build_convergence_record, build_failure_convergence_record};
use route_corrections::RouteCorrectionReader;
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
    /// Structured context payload (schema-versioned) for the council handshake.
    pub context_payload: Option<serde_json::Value>,
    /// Whether this run sits on the synchronous return path of a user prompt.
    /// Defaults to `false` for backward compatibility.
    pub critical_path: bool,
}

/// Apply route-correction weight adjustments to a route string.
///
/// Loads the [`RouteCorrectionReader`] and, for each `RouteId` that has a
/// non-zero cumulative weight, applies a bonus or penalty to the score used
/// for routing.
///
/// Returns the adjusted route string and the per-`RouteId` weight map.
pub fn apply_route_corrections(route: &str) -> (String, std::collections::HashMap<RouteId, f32>) {
    let reader = RouteCorrectionReader::new();
    let weights = reader.route_weights();

    // Map the input route string to a RouteId for weight lookup.
    // "direct" (council-only, no retrieval) → RouteId::CouncilOnly
    // "memory_only" → RouteId::CouncilWithMemory
    // "graph_only"  → RouteId::CouncilWithGraph
    // "both"        → RouteId::Both
    #[allow(clippy::match_same_arms)]
    let route_id = match route {
        "direct" | "council_only" => RouteId::CouncilOnly,
        "memory_only" | "memory" => RouteId::CouncilWithMemory,
        "graph_only" | "graph" => RouteId::CouncilWithGraph,
        "both" => RouteId::Both,
        _ => RouteId::CouncilOnly,
    };

    let weight = weights.get(&route_id).copied().unwrap_or(0.0_f32);

    // If the chosen route has been demoted below the threshold, switch to a
    // fallback that is less affected.  A weight below −0.3 signals chronic
    // quality issues on that route pattern.
    if weight < -0.3 {
        // Find the route with the highest remaining weight as fallback.
        let fallback = weights
            .iter()
            .filter(|(_, w)| **w > weight)
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(&id, _)| id);

        if let Some(fallback_id) = fallback {
            let adjusted_route = match fallback_id {
                RouteId::CouncilOnly => "direct",
                RouteId::CouncilWithMemory => "memory_only",
                RouteId::CouncilWithGraph => "graph_only",
                RouteId::Both => "both",
                _ => route,
            };
            return (adjusted_route.to_string(), weights);
        }
    }

    (route.to_string(), weights)
}

pub fn execute_council_run(request: CouncilRunRequest) -> Result<CouncilRunRecord> {
    let created_at = iso_now();
    let run_id = build_run_id(&request.task, &created_at);
    let artifacts_dir = request
        .artifacts_dir
        .clone()
        .unwrap_or_else(|| memoryport_dir().join("council-runs").join(&run_id));
    let run = initialize_run_record(request, run_id, created_at, &artifacts_dir)?;
    execute_council_run_from_state(run, &artifacts_dir)
}

pub fn resume_council_run(request: CouncilRunRequest) -> Result<CouncilRunRecord> {
    let artifacts_dir = request
        .artifacts_dir
        .clone()
        .context("resume requires an artifacts directory")?;
    let mut run = load_council_run_record_from_dir(&artifacts_dir)?;
    if run.status == "completed" {
        anyhow::bail!("run '{}' already completed", run.run_id);
    }
    if run.status == "failed" && run.status_reason == "artifact_validation_failed" {
        anyhow::bail!("run '{}' is corrupted and cannot be resumed", run.run_id);
    }

    // Inherit critical_path from the persisted run record if the resume
    // request doesn't explicitly set it.  This ensures a resumed critical
    // run keeps its priority scheduling.
    if request.critical_path {
        run.critical_path = true;
    }
    run.status = "running".to_string();
    run.status_reason = "resumed".to_string();
    run.retry_limit = request.retry_limit;
    run.timeout_secs = request.timeout_secs;
    run.updated_at = iso_now();
    run.convergence = None;
    persist_run_state(&artifacts_dir, &run)?;

    save_run_metadata(
        &artifacts_dir,
        &request.gemini_cmd,
        &request.claude_cmd,
        &request.codex_cmd,
        request.trace_path_override.as_deref(),
    )?;

    execute_council_run_from_state(run, &artifacts_dir)
}

fn initialize_run_record(
    request: CouncilRunRequest,
    run_id: String,
    created_at: String,
    artifacts_dir: &Path,
) -> Result<CouncilRunRecord> {
    let graph_context = request.graph_context.clone();
    fs::create_dir_all(artifacts_dir)?;

    let context_text_path = artifacts_dir.join("context.txt");
    let context_json_path = artifacts_dir.join("context.json");
    fs::write(&context_text_path, &request.context_text)?;
    fs::write(
        &context_json_path,
        serde_json::to_string_pretty(&request.context_json)?,
    )?;

    if let Some(payload) = &request.context_payload {
        if let Some(found_version) = payload
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
        {
            if !u32::try_from(found_version).is_ok_and(|v64| v64 == CONTEXT_PAYLOAD_SCHEMA_VERSION)
            {
                anyhow::bail!(
                    "unsupported context payload schema version: {found_version} (expected {CONTEXT_PAYLOAD_SCHEMA_VERSION})"
                );
            }
        }
        let payload_path = artifacts_dir.join("payload.json");
        fs::write(&payload_path, serde_json::to_string_pretty(payload)?)?;
    }

    save_run_metadata(
        artifacts_dir,
        &request.gemini_cmd,
        &request.claude_cmd,
        &request.codex_cmd,
        request.trace_path_override.as_deref(),
    )?;

    let run = CouncilRunRecord {
        run_id,
        task: request.task,
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
                artifacts_dir,
            ),
            initial_stage_record(
                "claude",
                "Claude",
                "Critique Gemini's draft and surface risks.",
                artifacts_dir,
            ),
            initial_stage_record(
                "codex",
                "Codex",
                "Converge on the smallest reliable executable outcome.",
                artifacts_dir,
            ),
        ],
        convergence: None,
        critical_path: request.critical_path,
    };
    persist_run_state(artifacts_dir, &run)?;
    Ok(run)
}

fn execute_council_run_from_state(
    mut run: CouncilRunRecord,
    artifacts_dir: &Path,
) -> Result<CouncilRunRecord> {
    let context_text = fs::read_to_string(&run.context_text_path)
        .with_context(|| format!("failed to read {}", run.context_text_path))?;
    let specs = build_stage_specs(&run);
    let start_index = run
        .stages
        .iter()
        .position(|stage| stage.status != "succeeded")
        .unwrap_or(run.stages.len());
    let mut prior_outputs = load_prior_outputs(&run)?;
    let mut circuit_breaker = from_env();

    // --- Critical-path routing: submit and acquire a worker slot -----------
    let dispatcher = crate::critical_path::global_dispatcher();
    let task_item = crate::critical_path::TaskItem::new(&run.run_id, run.critical_path);
    let enqueue_result = dispatcher.submit(task_item);

    if enqueue_result == crate::critical_path::EnqueueResult::BackpressureCritical {
        eprintln!(
            "[critical-path] back-pressure: critical queue full, run {} proceeding as standard",
            run.run_id
        );
    }

    // Try to acquire a worker slot.  If the pool is saturated the run
    // proceeds anyway — the slot accounting is best-effort for a CLI tool
    // where hard-blocking would be surprising.
    let slot = dispatcher.acquire();
    if let Some((ref _item, ref event)) = slot {
        eprintln!(
            "[critical-path] acquired slot for {} (priority={}, wait_ms={}, critical_depth={}, standard_depth={})",
            event.task_id, event.priority, event.wait_ms,
            event.critical_depth_after, event.standard_depth_after
        );
        log_dispatcher_event(artifacts_dir, event);
    }
    // Track whether we hold a slot so we can release it on all exit paths.
    let held_critical = slot.as_ref().map(|(item, _)| item.critical_path);
    // ----- end critical-path acquire --------------------------------------

    for (index, spec) in specs.iter().enumerate().skip(start_index) {
        let prompt = build_stage_prompt(
            spec.stage,
            spec.model,
            spec.role,
            &run.task,
            &context_text,
            run.graph_context.as_ref(),
            &prior_outputs,
        );
        let retry_limit = run.retry_limit;
        let timeout_secs = run.timeout_secs;
        let output = execute_stage(
            artifacts_dir,
            &mut run,
            index,
            spec,
            &prompt,
            retry_limit,
            timeout_secs,
        )?;

        let round_output = match &output {
            StageOutcome::Succeeded(s) => s.as_str(),
            StageOutcome::Failed { .. } => "",
        };
        if !circuit_breaker.record_round(round_output) {
            run.status = "failed".to_string();
            run.status_reason = format!(
                "circuit breaker tripped after {} no-progress rounds",
                circuit_breaker.consecutive_no_progress()
            );
            run.updated_at = iso_now();
            break;
        }

        match output {
            StageOutcome::Succeeded(output) => prior_outputs.push((spec.stage.to_string(), output)),
            StageOutcome::Failed { reason } => {
                run.status = "failed".to_string();
                run.status_reason = reason;
                run.updated_at = iso_now();
                break;
            }
        }
    }

    // --- Critical-path routing: release the worker slot -------------------
    if let Some(was_critical) = held_critical {
        dispatcher.release(was_critical);
        let (active_crit, active_std) = dispatcher.active_workers();
        eprintln!(
            "[critical-path] released slot for {} (active: critical={}, standard={})",
            run.run_id, active_crit, active_std
        );
    }

    finalize_council_run(artifacts_dir, run)
}

/// Append a dispatcher dequeue event to the run's artifacts for observability.
fn log_dispatcher_event(artifacts_dir: &Path, event: &crate::critical_path::DequeueEvent) {
    let path = artifacts_dir.join("dispatcher-events.jsonl");
    if let Ok(value) = serde_json::to_value(event) {
        let _ = crate::util::append_jsonl(&path, &value);
    }
}

fn build_stage_specs(run: &CouncilRunRecord) -> [StageSpec<'static>; 3] {
    [
        StageSpec {
            stage: "gemini",
            model: "Gemini",
            role: "Generate options before convergence.",
            command: Box::leak(resolve_stage_command(run, "gemini").into_boxed_str()),
        },
        StageSpec {
            stage: "claude",
            model: "Claude",
            role: "Critique Gemini's draft and surface risks.",
            command: Box::leak(resolve_stage_command(run, "claude").into_boxed_str()),
        },
        StageSpec {
            stage: "codex",
            model: "Codex",
            role: "Converge on the smallest reliable executable outcome.",
            command: Box::leak(resolve_stage_command(run, "codex").into_boxed_str()),
        },
    ]
}

fn resolve_stage_command(run: &CouncilRunRecord, stage: &str) -> String {
    let key = format!("command_{stage}");
    let metadata_path = PathBuf::from(&run.artifacts_dir).join("run-metadata.json");
    if let Ok(text) = fs::read_to_string(&metadata_path)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(cmd) = value.get(&key).and_then(serde_json::Value::as_str)
    {
        return cmd.to_string();
    }
    std::env::var(format!("LAYERS_COUNCIL_{}_CMD", stage.to_uppercase())).unwrap_or_default()
}

fn load_prior_outputs(run: &CouncilRunRecord) -> Result<Vec<(String, String)>> {
    let mut prior_outputs = Vec::new();
    for stage in &run.stages {
        if stage.status == "succeeded" && !stage.output_path.is_empty() {
            let output = fs::read_to_string(&stage.output_path)
                .with_context(|| format!("failed to read {}", stage.output_path))?;
            prior_outputs.push((stage.stage.clone(), output));
        }
    }
    Ok(prior_outputs)
}

fn finalize_council_run(
    artifacts_dir: &Path,
    mut run: CouncilRunRecord,
) -> Result<CouncilRunRecord> {
    let prior_outputs = load_prior_outputs(&run)?;
    let convergence = if run.status == "failed" {
        build_failure_convergence_record(artifacts_dir, &run)?
    } else {
        let final_output = prior_outputs
            .last()
            .map(|(_, output)| output.as_str())
            .unwrap_or_default();
        let final_output_path = run
            .stages
            .iter()
            .rev()
            .find(|stage| stage.status == "succeeded")
            .map(|stage| stage.output_path.clone())
            .unwrap_or_default();
        build_convergence_record(artifacts_dir, final_output, &final_output_path)?
    };

    if run.status != "failed" {
        if convergence.status == "converged" {
            run.status = "completed".to_string();
            run.status_reason = "converged".to_string();
        } else {
            run.status = "incomplete".to_string();
            run.status_reason.clone_from(&convergence.reason);
        }
    }

    run.updated_at = iso_now();
    run.convergence = Some(convergence.clone());
    run.artifact_errors = validate_run_artifacts(artifacts_dir, &run);
    if !run.artifact_errors.is_empty() {
        run.status = "failed".to_string();
        run.status_reason = "artifact_validation_failed".to_string();
        if let Some(item) = run.convergence.as_mut() {
            item.status = "not_converged".to_string();
            item.reason = "artifact_validation_failed".to_string();
        }
    }
    persist_run_state(artifacts_dir, &run)?;
    let trace_convergence = run.convergence.as_ref().unwrap_or(&convergence);
    let trace_path = load_trace_path(artifacts_dir);
    append_trace_record(&run, trace_convergence, trace_path.as_deref())?;
    Ok(run)
}

fn save_run_metadata(
    artifacts_dir: &Path,
    gemini_cmd: &str,
    claude_cmd: &str,
    codex_cmd: &str,
    trace_path_override: Option<&Path>,
) -> Result<()> {
    let metadata = serde_json::json!({
        "command_gemini": gemini_cmd,
        "command_claude": claude_cmd,
        "command_codex": codex_cmd,
        "trace_path": trace_path_override.map(|path| path.display().to_string()),
    });
    fs::write(
        artifacts_dir.join("run-metadata.json"),
        serde_json::to_string_pretty(&metadata)?,
    )?;
    Ok(())
}

fn load_trace_path(artifacts_dir: &Path) -> Option<PathBuf> {
    let metadata_path = artifacts_dir.join("run-metadata.json");
    let text = fs::read_to_string(metadata_path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    value
        .get("trace_path")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

pub fn default_run_artifacts_dir(run_id: &str) -> PathBuf {
    memoryport_dir().join("council-runs").join(run_id)
}

pub fn load_council_run_record(
    run_id: &str,
    artifacts_dir_override: Option<&Path>,
) -> Result<CouncilRunRecord> {
    let artifacts_dir =
        artifacts_dir_override.map_or_else(|| default_run_artifacts_dir(run_id), Path::to_path_buf);
    let record = load_council_run_record_from_dir(&artifacts_dir)?;
    if record.run_id != run_id {
        anyhow::bail!(
            "run id mismatch: requested '{}' but artifact contains '{}'",
            run_id,
            record.run_id
        );
    }
    Ok(record)
}

fn load_council_run_record_from_dir(artifacts_dir: &Path) -> Result<CouncilRunRecord> {
    let run_path = artifacts_dir.join("run.json");
    let run = fs::read_to_string(&run_path)
        .with_context(|| format!("failed to read {}", run_path.display()))?;
    serde_json::from_str::<CouncilRunRecord>(&run)
        .with_context(|| format!("failed to parse {}", run_path.display()))
}

pub fn load_council_convergence_record(
    run: &CouncilRunRecord,
    artifacts_dir_override: Option<&Path>,
) -> Result<CouncilConvergenceRecord> {
    let artifacts_dir =
        artifacts_dir_override.map_or_else(|| PathBuf::from(&run.artifacts_dir), Path::to_path_buf);
    let convergence_path = artifacts_dir.join("convergence.json");
    let convergence = fs::read_to_string(&convergence_path)
        .with_context(|| format!("failed to read {}", convergence_path.display()))?;
    serde_json::from_str::<CouncilConvergenceRecord>(&convergence)
        .with_context(|| format!("failed to parse {}", convergence_path.display()))
}

pub fn load_council_checkpoint(
    run_id: &str,
    artifacts_dir_override: Option<&Path>,
) -> Result<crate::types::CouncilRunCheckpoint> {
    let artifacts_dir =
        artifacts_dir_override.map_or_else(|| default_run_artifacts_dir(run_id), Path::to_path_buf);
    let checkpoint_path = artifacts_dir.join("checkpoint.json");
    let checkpoint = fs::read_to_string(&checkpoint_path)
        .with_context(|| format!("failed to read {}", checkpoint_path.display()))?;
    serde_json::from_str::<crate::types::CouncilRunCheckpoint>(&checkpoint)
        .with_context(|| format!("failed to parse {}", checkpoint_path.display()))
}

pub fn list_council_runs(limit: usize) -> Result<Vec<CouncilRunRecord>> {
    let root = memoryport_dir().join("council-runs");
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut runs = fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false))
        .filter_map(|entry| {
            let run_id = entry.file_name().to_string_lossy().to_string();
            load_council_run_record(&run_id, Some(&entry.path())).ok()
        })
        .collect::<Vec<_>>();
    runs.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    if runs.len() > limit {
        runs.truncate(limit);
    }
    Ok(runs)
}

pub fn latest_incomplete_council_run() -> Result<Option<CouncilRunRecord>> {
    let run = list_council_runs(50)?
        .into_iter()
        .find(|run| run.status != "completed");
    Ok(run)
}

fn build_stage_prompt(
    stage: &str,
    model: &str,
    role: &str,
    task: &str,
    context_text: &str,
    graph_context: Option<&ImpactSummary>,
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
            prompt.push(output.clone());
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
            context_payload: None,
            critical_path: false,
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
            "count=0; [ -f '{counter_text}' ] && count=$(cat '{counter_text}'); count=$((count+1)); echo $count > '{counter_text}'; if [ \"$count\" -eq 1 ]; then echo 'first failure' >&2; exit 1; fi; printf '## Decision\\n- retry worked with a real contract\\n## Why\\n- the second attempt reused the same grounded task\\n## Risks\\n- minor residual risk\\n## Next Steps\\n- keep the final artifacts\\nConvergence: converged\\n'",
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
            context_payload: None,
            critical_path: false,
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
            context_payload: None,
            critical_path: false,
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
            context_payload: None,
            critical_path: false,
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
    fn council_handshake_writes_versioned_payload_json() {
        use crate::config::CONTEXT_PAYLOAD_SCHEMA_VERSION;

        let _guard = workspace_guard();
        let artifacts_dir = temp_artifact_dir("council-handshake");

        let payload = json!({
            "schema_version": CONTEXT_PAYLOAD_SCHEMA_VERSION,
            "task": "handshake test",
            "route": "both",
            "confidence": "high",
            "memory_results": [],
            "graph_results": [],
            "retrieval_meta": {
                "memory_source": "none",
                "memory_latency_ms": 0,
                "graph_latency_ms": 0,
                "fallback_reason": null,
            },
        });

        let request = CouncilRunRequest {
            task: "Validate council handshake writes payload.json".to_string(),
            route: "both".to_string(),
            context_text: "Route: both\nEvidence: handshake test".to_string(),
            context_json: json!({"route": "both"}),
            graph_context: None,
            targets: vec![],
            gemini_cmd: "printf '## Options\n- option a\n## Key Evidence\n- evidence\n## Open Questions\n- none\n'".to_string(),
            claude_cmd: "printf '## Critique\n- looks good\n## Risks\n- none\n## Best Surviving Direction\n- option a\n'".to_string(),
            codex_cmd: "printf '## Decision\n- proceed with option a\n## Why\n- grounded in evidence\n## Risks\n- minimal\n## Next Steps\n- ship it\nConvergence: converged\n'".to_string(),
            retry_limit: 1,
            timeout_secs: 2,
            artifacts_dir: Some(artifacts_dir.clone()),
            trace_path_override: Some(artifacts_dir.join("council-traces.jsonl")),
            context_payload: Some(payload),
            critical_path: false,
        };

        let run = execute_council_run(request).unwrap();

        // The run should complete successfully
        assert_eq!(run.status, "completed");
        assert_eq!(run.status_reason, "converged");

        // payload.json must exist in the artifacts directory
        let payload_path = artifacts_dir.join("payload.json");
        assert!(
            payload_path.exists(),
            "payload.json was not written to artifacts dir"
        );

        // Parse and validate schema version
        let payload_content = fs::read_to_string(&payload_path).unwrap();
        let payload_value: serde_json::Value =
            serde_json::from_str(&payload_content).expect("payload.json is not valid JSON");
        assert_eq!(
            payload_value["schema_version"],
            json!(CONTEXT_PAYLOAD_SCHEMA_VERSION),
            "payload.json schema_version mismatch"
        );
        assert_eq!(payload_value["task"], "handshake test");
        assert_eq!(payload_value["route"], "both");

        // run.json and convergence.json must also exist (standard artifacts)
        assert!(artifacts_dir.join("run.json").exists());
        assert!(artifacts_dir.join("convergence.json").exists());
    }

    #[test]
    fn council_handshake_rejects_wrong_schema_version() {
        let _guard = workspace_guard();
        let artifacts_dir = temp_artifact_dir("council-bad-schema");

        let bad_payload = json!({
            "schema_version": 999,
            "task": "bad version",
        });

        let request = CouncilRunRequest {
            task: "Schema version mismatch should fail".to_string(),
            route: "both".to_string(),
            context_text: "Route: both".to_string(),
            context_json: json!({"route": "both"}),
            graph_context: None,
            targets: vec![],
            gemini_cmd: "printf 'unused\n'".to_string(),
            claude_cmd: "printf 'unused\n'".to_string(),
            codex_cmd: "printf 'unused\n'".to_string(),
            retry_limit: 1,
            timeout_secs: 2,
            artifacts_dir: Some(artifacts_dir),
            trace_path_override: None,
            context_payload: Some(bad_payload),
            critical_path: false,
        };

        let result = execute_council_run(request);
        assert!(result.is_err(), "expected error for wrong schema version");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("unsupported context payload schema version"),
            "unexpected error: {msg}",
        );
    }

    #[test]
    fn council_run_writes_checkpoint_file() {
        let _guard = workspace_guard();
        let artifacts_dir = temp_artifact_dir("council-checkpoint");
        let request = CouncilRunRequest {
            task: "Checkpoint after each stage".to_string(),
            route: "direct".to_string(),
            context_text: "Route: direct".to_string(),
            context_json: json!({"route": "direct"}),
            graph_context: None,
            targets: vec![],
            gemini_cmd: "printf '## Options\n- one\n## Key Evidence\n- evidence\n## Open Questions\n- none\n'".to_string(),
            claude_cmd: "printf '## Critique\n- acceptable\n## Risks\n- low\n## Best Surviving Direction\n- keep going\n'".to_string(),
            codex_cmd: "printf '## Decision\n- proceed\n## Why\n- durable\n## Risks\n- low\n## Next Steps\n- ship\nConvergence: converged\n'".to_string(),
            retry_limit: 1,
            timeout_secs: 2,
            artifacts_dir: Some(artifacts_dir.clone()),
            trace_path_override: None,
            context_payload: None,
            critical_path: false,
        };

        let run = execute_council_run(request).unwrap();
        let checkpoint = load_council_checkpoint(&run.run_id, Some(&artifacts_dir)).unwrap();
        assert!(artifacts_dir.join("checkpoint.json").exists());
        assert_eq!(checkpoint.current_stage_index, 3);
        assert_eq!(checkpoint.status, "completed");
        assert_eq!(checkpoint.stages_completed.len(), 3);
    }

    #[test]
    fn council_run_can_resume_from_failed_stage() {
        let _guard = workspace_guard();
        let artifacts_dir = temp_artifact_dir("council-resume");
        let request = CouncilRunRequest {
            task: "Resume from last good stage".to_string(),
            route: "direct".to_string(),
            context_text: "Route: direct".to_string(),
            context_json: json!({"route": "direct"}),
            graph_context: None,
            targets: vec![],
            gemini_cmd: "printf '## Options\n- keep prior work\n## Key Evidence\n- checkpoint exists\n## Open Questions\n- resume next stage\n'".to_string(),
            claude_cmd: "printf 'bad\n'".to_string(),
            codex_cmd: "printf '## Decision\n- should not run\n## Why\n- no\n## Risks\n- no\n## Next Steps\n- no\nConvergence: converged\n'".to_string(),
            retry_limit: 1,
            timeout_secs: 2,
            artifacts_dir: Some(artifacts_dir.clone()),
            trace_path_override: None,
            context_payload: None,
            critical_path: false,
        };

        let failed = execute_council_run(request).unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.stages[0].status, "succeeded");
        assert_eq!(failed.stages[1].status, "failed");
        assert_eq!(failed.stages[2].status, "pending");

        let resumed = resume_council_run(CouncilRunRequest {
            task: String::new(),
            route: String::new(),
            context_text: String::new(),
            context_json: json!({}),
            graph_context: None,
            targets: vec![],
            gemini_cmd: "printf '## Options\n- should not rerun\n## Key Evidence\n- no\n## Open Questions\n- no\n'".to_string(),
            claude_cmd: "printf '## Critique\n- fixed\n## Risks\n- manageable\n## Best Surviving Direction\n- continue\n'".to_string(),
            codex_cmd: "printf '## Decision\n- resume worked\n## Why\n- gemini stayed intact\n## Risks\n- low\n## Next Steps\n- keep checkpointing\nConvergence: converged\n'".to_string(),
            retry_limit: 1,
            timeout_secs: 2,
            artifacts_dir: Some(artifacts_dir.clone()),
            trace_path_override: None,
            context_payload: None,
            critical_path: false,
        })
        .unwrap();

        assert_eq!(resumed.status, "completed");
        assert_eq!(resumed.stages[0].attempts.len(), 1);
        assert_eq!(resumed.stages[1].status, "succeeded");
        assert_eq!(resumed.stages[2].status, "succeeded");
        let checkpoint = load_council_checkpoint(&resumed.run_id, Some(&artifacts_dir)).unwrap();
        assert_eq!(checkpoint.current_stage_index, 3);
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
            context_payload: None,
            critical_path: false,
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
