//! Workflow benchmark telemetry analysis for comparing Layers vs baseline runs.
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use clap::Subcommand;
use serde::{Deserialize, Serialize};

/// Workflow benchmark commands.
#[derive(Debug, Subcommand)]
pub(crate) enum WorkflowBenchmarkCommands {
    /// Analyze Layers-vs-baseline workflow telemetry from JSONL.
    Analyze {
        /// Path to workflow run telemetry JSONL.
        path: PathBuf,
        /// Output a structured JSON report.
        #[arg(long)]
        json: bool,
    },
    /// Validate preregistered workflow benchmark task specs.
    ValidateTasks {
        /// Task spec file or directory containing task spec JSON files.
        path: PathBuf,
        /// Output a structured JSON validation report.
        #[arg(long)]
        json: bool,
    },
    /// Export task/file relevance data for retrieval evaluation.
    RetrievalEvalCorpus {
        /// Task spec file or directory containing task spec JSON files.
        path: PathBuf,
        /// Repository root containing expected relevant files.
        #[arg(long, default_value = ".")]
        repo_root: PathBuf,
        /// Output a structured JSON retrieval corpus.
        #[arg(long)]
        json: bool,
    },
    /// Score a retrieval eval corpus with the deterministic lexical baseline.
    RetrievalEvalLexical {
        /// Retrieval eval corpus JSON produced by `retrieval-eval-corpus`.
        path: PathBuf,
        /// Output a structured JSON retrieval report.
        #[arg(long)]
        json: bool,
    },
    /// Plan isolated paired coding-agent benchmark runs without executing them.
    PlanRun {
        /// Task spec file or directory containing task spec JSON files.
        path: PathBuf,
        /// Output artifact directory for prompts, order, and runner plan JSON.
        #[arg(long)]
        output_dir: PathBuf,
        /// Repository root to clone/reset from when executing the plan.
        #[arg(long, default_value = ".")]
        repo_root: PathBuf,
        /// Agent command recorded in each run plan, e.g. `codex exec` or `claude -p`.
        #[arg(long)]
        agent_command: String,
        /// Optional model name recorded in each run plan.
        #[arg(long)]
        model: Option<String>,
        /// Deterministic seed for variant/task execution order.
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// Output the structured runner plan JSON to stdout.
        #[arg(long)]
        json: bool,
    },
    /// Execute a runner plan in isolated worktrees and emit benchmark run records.
    RunPlan {
        /// Path to runner-plan.json produced by `workflow-benchmark plan-run`.
        path: PathBuf,
        /// Targeted preflight command to run before targeted-preflight agent runs.
        #[arg(long, default_value = "layers preflight --no-audit --json --strict")]
        preflight_command: String,
        /// Keep worktrees after execution for manual inspection.
        #[arg(long)]
        keep_worktrees: bool,
        /// Output the structured execution report JSON to stdout.
        #[arg(long)]
        json: bool,
    },
}

/// Handle workflow benchmark commands.
pub(crate) fn handle_workflow_benchmark(command: &WorkflowBenchmarkCommands) -> Result<()> {
    match command {
        WorkflowBenchmarkCommands::Analyze { path, json } => {
            let runs = load_runs(path)?;
            let report = analyze_runs_with_thresholds(&runs, ClaimThresholds::default())?;
            println!("{}", format_report(&report, *json)?);
            Ok(())
        }
        WorkflowBenchmarkCommands::ValidateTasks { path, json } => {
            let report = validate_task_specs(path)?;
            println!("{}", format_task_validation_report(&report, *json)?);
            if report.invalid_count > 0 {
                bail!(
                    "{} workflow task spec(s) failed validation",
                    report.invalid_count
                );
            }
            Ok(())
        }
        WorkflowBenchmarkCommands::RetrievalEvalCorpus {
            path,
            repo_root,
            json,
        } => {
            let corpus = build_retrieval_eval_corpus(&RetrievalEvalConfig {
                task_path: path.clone(),
                repo_root: repo_root.clone(),
            })?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&corpus)
                        .context("failed to serialize retrieval eval corpus")?
                );
            } else {
                println!("Workflow retrieval eval corpus");
                println!("pairs: {}", corpus.pairs.len());
                println!("documents: {}", corpus.documents.len());
                println!("negative_controls: {}", corpus.negative_control_count);
            }
            Ok(())
        }
        WorkflowBenchmarkCommands::RetrievalEvalLexical { path, json } => {
            let corpus = load_retrieval_eval_corpus(path)?;
            let report = evaluate_lexical_retrieval(&corpus)?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .context("failed to serialize lexical retrieval report")?
                );
            } else {
                println!("Workflow lexical retrieval eval");
                println!("pairs: {}", report.pair_count);
                println!("documents: {}", report.document_count);
                println!("recall@5: {:.4}", report.recall_at_5);
                println!("recall@10: {:.4}", report.recall_at_10);
                println!("mrr: {:.4}", report.mrr);
            }
            Ok(())
        }
        WorkflowBenchmarkCommands::PlanRun {
            path,
            output_dir,
            repo_root,
            agent_command,
            model,
            seed,
            json,
        } => {
            let config = RunnerPlanConfig {
                task_path: path.clone(),
                output_dir: output_dir.clone(),
                repo_root: repo_root.clone(),
                agent_command: agent_command.clone(),
                model: model.clone(),
                seed: *seed,
            };
            let plan = plan_runner_artifacts(&config)?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&plan)
                        .context("failed to serialize runner plan")?
                );
            } else {
                println!("Workflow benchmark runner plan");
                println!("tasks: {}", plan.task_count);
                println!("runs: {}", plan.runs.len());
                println!("output_dir: {}", plan.output_dir.display());
                println!("runner_plan: {}", plan.plan_path.display());
                println!("execution_order: {}", plan.execution_order_path.display());
            }
            Ok(())
        }
        WorkflowBenchmarkCommands::RunPlan {
            path,
            preflight_command,
            keep_worktrees,
            json,
        } => {
            let report = execute_runner_plan(&RunnerExecutionConfig {
                plan_path: path.clone(),
                preflight_command: preflight_command.clone(),
                keep_worktrees: *keep_worktrees,
            })?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .context("failed to serialize runner execution report")?
                );
            } else {
                println!("Workflow benchmark runner execution");
                println!("runs: {}", report.total_runs);
                println!("completed: {}", report.completed_runs);
                println!("failed: {}", report.failed_runs);
                println!("run_records: {}", report.run_records_path.display());
                println!(
                    "execution_report: {}",
                    report.execution_report_path.display()
                );
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkflowVariant {
    Baseline,
    #[serde(alias = "layers")]
    LayersTargetedPreflight,
    LayersBroadQuery,
    LayersMcpPreflight,
}

impl WorkflowVariant {
    fn is_layers(self) -> bool {
        !matches!(self, Self::Baseline)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RetrievalQuality {
    relevance: u8,
    completeness: u8,
    specificity: u8,
    freshness: u8,
    grounding: u8,
    concision: u8,
    noise_absence: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct WorkflowRun {
    task_id: String,
    variant: WorkflowVariant,
    task_category: String,
    success_score: f64,
    wall_time_ms: u64,
    orientation_ms: u64,
    implementation_ms: u64,
    debugging_ms: u64,
    verification_ms: u64,
    input_tokens: u64,
    output_tokens: u64,
    peak_context_tokens: u64,
    context_relevant_tokens: u64,
    context_duplicate_tokens: u64,
    context_irrelevant_tokens: u64,
    assistant_turns: u64,
    tool_calls: u64,
    failed_commands: u64,
    patch_attempts: u64,
    test_runs: u64,
    human_interventions: u64,
    failed_attempts: u64,
    retrieval_quality: RetrievalQuality,
    verification_quality: u8,
    change_quality: u8,
    planning_quality: u8,
    reproducibility: u8,
    confidence_calibration: u8,
    user_usefulness: u8,
    layers_overhead_ms: u64,
    layers_overhead_tokens: u64,
    missed_critical_context: u64,
    hallucinated_or_stale_context: u64,
    regressions: u64,
    #[serde(default)]
    negative_control_abstained: bool,
    #[serde(default)]
    unnecessary_context_injections: u64,
    #[serde(default)]
    context_caused_regressions: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SurfaceClaim {
    #[default]
    LayersTargetedPreflight,
    LayersBroadQuery,
    LayersMcpPreflight,
    Baseline,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SuccessRubric {
    pub(crate) full_success: String,
    pub(crate) partial_success: String,
    pub(crate) failure: String,
    pub(crate) min_verification_quality: u8,
    #[serde(default = "default_primary_endpoint")]
    pub(crate) primary_endpoint: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskSpec {
    pub(crate) task_id: String,
    pub(crate) title: String,
    pub(crate) prompt: String,
    pub(crate) category: String,
    #[serde(default)]
    pub(crate) difficulty: Option<String>,
    #[serde(default)]
    pub(crate) surface_claim: SurfaceClaim,
    #[serde(default)]
    pub(crate) negative_control: bool,
    #[serde(default)]
    pub(crate) stale_context_trap: bool,
    #[serde(default)]
    pub(crate) repo_commit: Option<String>,
    #[serde(default)]
    pub(crate) time_budget_minutes: Option<u64>,
    #[serde(default)]
    pub(crate) target_files: Vec<String>,
    #[serde(default)]
    pub(crate) target_symbols: Vec<String>,
    #[serde(default)]
    pub(crate) expected_relevant_files: Vec<String>,
    pub(crate) expected_validation_commands: Vec<String>,
    pub(crate) success_rubric: SuccessRubric,
    #[serde(default)]
    pub(crate) abstention_rubric: Option<String>,
}

fn default_primary_endpoint() -> String {
    "verified_success".to_owned()
}

#[derive(Debug, Clone, Serialize)]
struct TaskValidationReport {
    checked_count: usize,
    valid_count: usize,
    invalid_count: usize,
    results: Vec<TaskValidationResult>,
}

#[derive(Debug, Clone, Serialize)]
struct TaskValidationResult {
    path: PathBuf,
    task_id: Option<String>,
    valid: bool,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
struct RunnerPlanConfig {
    task_path: PathBuf,
    output_dir: PathBuf,
    repo_root: PathBuf,
    agent_command: String,
    model: Option<String>,
    seed: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RunnerPlan {
    task_count: usize,
    variants: Vec<String>,
    repo_root: PathBuf,
    output_dir: PathBuf,
    worktree_root: PathBuf,
    #[serde(rename = "runner_plan_path")]
    plan_path: PathBuf,
    execution_order_path: PathBuf,
    seed: u64,
    runs: Vec<RunnerRunPlan>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RunnerRunPlan {
    task_id: String,
    task_category: String,
    variant: String,
    run_id: String,
    worktree_path: PathBuf,
    prompt_path: PathBuf,
    transcript_path: PathBuf,
    validation_log_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    packet_artifact_path: Option<PathBuf>,
    requires_layers_preflight: bool,
    agent_command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    time_budget_minutes: u64,
    expected_validation_commands: Vec<String>,
}

#[derive(Debug, Clone)]
struct RunnerExecutionConfig {
    plan_path: PathBuf,
    preflight_command: String,
    keep_worktrees: bool,
}

#[derive(Debug, Clone)]
struct RetrievalEvalConfig {
    task_path: PathBuf,
    repo_root: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RetrievalEvalCorpus {
    pairs: Vec<RetrievalEvalPair>,
    documents: Vec<RetrievalEvalDocument>,
    negative_control_count: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RetrievalEvalPair {
    task_id: String,
    query: String,
    relevant_ids: Vec<String>,
    category: String,
    stale_context_trap: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RetrievalEvalDocument {
    id: String,
    source_kind: String,
    path: Option<String>,
    text: String,
}

#[derive(Debug, Clone, Serialize)]
struct LexicalRetrievalReport {
    pair_count: usize,
    document_count: usize,
    negative_control_count: usize,
    recall_at_5: f64,
    recall_at_10: f64,
    mrr: f64,
    negative_control_injection_rate: f64,
    per_pair: Vec<LexicalPairReport>,
}

#[derive(Debug, Clone, Serialize)]
struct LexicalPairReport {
    task_id: String,
    top_document_ids: Vec<String>,
    first_relevant_rank: Option<usize>,
    recall_at_5: f64,
    recall_at_10: f64,
}

#[derive(Debug, Clone, Serialize)]
struct RunnerExecutionSummary {
    total_runs: usize,
    completed_runs: usize,
    failed_runs: usize,
    run_records_path: PathBuf,
    execution_report_path: PathBuf,
    runs: Vec<RunnerRunExecution>,
}

#[derive(Debug, Clone, Serialize)]
struct RunnerRunExecution {
    run_id: String,
    task_id: String,
    variant: String,
    worktree_path: PathBuf,
    transcript_path: PathBuf,
    validation_log_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    packet_artifact_path: Option<PathBuf>,
    agent_exit_code: Option<i32>,
    validation_exit_codes: Vec<Option<i32>>,
    wall_time_ms: u64,
    completed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct BenchmarkReport {
    run_count: usize,
    paired_task_count: usize,
    variants: Vec<VariantAggregate>,
    baseline: Option<VariantAggregate>,
    layers: Option<VariantAggregate>,
    comparisons: Vec<PairedComparison>,
    comparison: Option<PairedComparison>,
    #[serde(skip_serializing_if = "Option::is_none")]
    claim: Option<ClaimReport>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
struct ClaimThresholds {
    min_paired_tasks: usize,
    min_code_heavy_paired_tasks: usize,
    min_negative_control_paired_tasks: usize,
    min_success_delta: f64,
    min_time_saved_ms: f64,
    min_token_reduction_ratio: f64,
    max_missed_critical_context_rate: f64,
    max_hallucinated_or_stale_context_rate: f64,
    max_regression_rate: f64,
    max_context_caused_regression_rate: f64,
    min_negative_control_abstention_rate: f64,
    max_unnecessary_context_injection_rate: f64,
}

impl Default for ClaimThresholds {
    fn default() -> Self {
        Self {
            min_paired_tasks: 30,
            min_code_heavy_paired_tasks: 20,
            min_negative_control_paired_tasks: 5,
            min_success_delta: 0.0,
            min_time_saved_ms: 0.0,
            min_token_reduction_ratio: 0.20,
            max_missed_critical_context_rate: 0.05,
            max_hallucinated_or_stale_context_rate: 0.0,
            max_regression_rate: 0.0,
            max_context_caused_regression_rate: 0.0,
            min_negative_control_abstention_rate: 0.95,
            max_unnecessary_context_injection_rate: 0.05,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ClaimStatus {
    Supported,
    NotSupported,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize)]
struct ClaimReport {
    status: ClaimStatus,
    thresholds: ClaimThresholds,
    blocking_metrics: Vec<String>,
    uncertainty_notes: Vec<String>,
    code_heavy_paired_task_count: usize,
    negative_control_paired_task_count: usize,
    negative_control_abstention_rate: f64,
    unnecessary_context_injection_rate: f64,
    context_caused_regression_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
struct VariantAggregate {
    variant: WorkflowVariant,
    run_count: usize,
    success_rate: f64,
    median_wall_time_ms: f64,
    average_wall_time_ms: f64,
    average_orientation_ms: f64,
    average_debugging_ms: f64,
    average_verification_ms: f64,
    average_input_tokens: f64,
    average_output_tokens: f64,
    average_total_tokens: f64,
    average_tool_calls: f64,
    average_human_interventions: f64,
    average_failed_attempts: f64,
    average_verification_quality: f64,
    average_change_quality: f64,
    average_planning_quality: f64,
    average_user_usefulness: f64,
    retrieval_quality_average: f64,
    relevant_context_ratio: f64,
    context_waste_ratio: f64,
    missed_critical_context_rate: f64,
    hallucinated_or_stale_context_rate: f64,
    regression_rate: f64,
    average_layers_overhead_ms: f64,
    average_layers_overhead_tokens: f64,
}

#[derive(Debug, Clone, Serialize)]
struct PairedComparison {
    variant: WorkflowVariant,
    paired_task_count: usize,
    net_time_saved_ms: f64,
    net_tokens_saved: f64,
    speedup: f64,
    token_reduction_ratio: f64,
    success_delta: f64,
    success_delta_confidence_interval: ConfidenceInterval,
    human_intervention_delta: f64,
    tool_call_delta: f64,
    failed_attempt_delta: f64,
    verification_quality_delta: f64,
    context_quality_delta: f64,
    layers_overhead_ms: f64,
    layers_overhead_tokens: f64,
    missed_critical_context_rate: f64,
    hallucinated_or_stale_context_rate: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ConfidenceInterval {
    estimate: f64,
    lower_bound: f64,
    upper_bound: f64,
    confidence_level: f64,
}

#[derive(Debug, Clone)]
struct TaskRunAverage {
    wall_time_ms: f64,
    total_tokens: f64,
    success_score: f64,
    human_interventions: f64,
    tool_calls: f64,
    failed_attempts: f64,
    verification_quality: f64,
    retrieval_quality: f64,
    layers_overhead_ms: f64,
    layers_overhead_tokens: f64,
    missed_critical_context: f64,
    hallucinated_or_stale_context: f64,
}

fn validate_task_specs(path: &Path) -> Result<TaskValidationReport> {
    let mut paths = collect_task_spec_paths(path)?;
    paths.sort();

    let mut results = Vec::with_capacity(paths.len());
    for path in paths {
        match load_task_spec_unchecked(&path) {
            Ok(spec) => match validate_task_spec(&spec) {
                Ok(()) => results.push(TaskValidationResult {
                    path,
                    task_id: Some(spec.task_id),
                    valid: true,
                    errors: Vec::new(),
                }),
                Err(error) => results.push(TaskValidationResult {
                    path,
                    task_id: Some(spec.task_id),
                    valid: false,
                    errors: vec![error.to_string()],
                }),
            },
            Err(error) => results.push(TaskValidationResult {
                path,
                task_id: None,
                valid: false,
                errors: vec![format!("{error:?}")],
            }),
        }
    }

    let checked_count = results.len();
    let invalid_count = results.iter().filter(|result| !result.valid).count();
    let valid_count = checked_count.saturating_sub(invalid_count);

    Ok(TaskValidationReport {
        checked_count,
        valid_count,
        invalid_count,
        results,
    })
}

fn collect_task_spec_paths(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        bail!("task spec path does not exist: {}", path.display());
    }

    let mut paths = Vec::new();
    collect_task_spec_paths_recursive(path, &mut paths)?;
    if paths.is_empty() {
        bail!("no task spec JSON files found under {}", path.display());
    }
    Ok(paths)
}

fn collect_task_spec_paths_recursive(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read task spec directory: {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_task_spec_paths_recursive(&path, paths)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn build_retrieval_eval_corpus(config: &RetrievalEvalConfig) -> Result<RetrievalEvalCorpus> {
    let mut task_paths = collect_task_spec_paths(&config.task_path)?;
    task_paths.sort();

    let mut pairs = Vec::new();
    let mut documents_by_id = BTreeMap::<String, RetrievalEvalDocument>::new();
    let mut negative_control_count = 0usize;

    for task_path in task_paths {
        let spec = load_task_spec_unchecked(&task_path)?;
        validate_task_spec(&spec)
            .with_context(|| format!("invalid task spec: {}", task_path.display()))?;
        if spec.negative_control {
            negative_control_count += 1;
            continue;
        }

        let relevant_ids = spec
            .expected_relevant_files
            .iter()
            .map(|path| file_document_id(path))
            .collect::<Vec<_>>();
        for path in &spec.expected_relevant_files {
            add_file_document(&mut documents_by_id, &config.repo_root, path)?;
        }
        pairs.push(RetrievalEvalPair {
            task_id: spec.task_id,
            query: spec.prompt,
            relevant_ids,
            category: spec.category,
            stale_context_trap: spec.stale_context_trap,
        });
    }

    Ok(RetrievalEvalCorpus {
        pairs,
        documents: documents_by_id.into_values().collect(),
        negative_control_count,
    })
}

fn add_file_document(
    documents_by_id: &mut BTreeMap<String, RetrievalEvalDocument>,
    repo_root: &Path,
    repo_path: &str,
) -> Result<()> {
    let id = file_document_id(repo_path);
    if documents_by_id.contains_key(&id) {
        return Ok(());
    }
    let absolute_path = repo_root.join(repo_path);
    let text = fs::read_to_string(&absolute_path).with_context(|| {
        format!(
            "failed to read retrieval eval document: {}",
            absolute_path.display()
        )
    })?;
    documents_by_id.insert(
        id.clone(),
        RetrievalEvalDocument {
            id,
            source_kind: "file".to_string(),
            path: Some(repo_path.to_string()),
            text,
        },
    );
    Ok(())
}

fn file_document_id(repo_path: &str) -> String {
    format!("file:{repo_path}")
}

fn load_retrieval_eval_corpus(path: &Path) -> Result<RetrievalEvalCorpus> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read retrieval eval corpus: {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| {
        format!(
            "retrieval eval corpus is not valid JSON: {}",
            path.display()
        )
    })
}

fn evaluate_lexical_retrieval(corpus: &RetrievalEvalCorpus) -> Result<LexicalRetrievalReport> {
    if corpus.pairs.is_empty() {
        bail!("retrieval eval corpus must contain at least one non-negative-control pair");
    }
    if corpus.documents.is_empty() {
        bail!("retrieval eval corpus must contain at least one document");
    }

    let mut per_pair = Vec::with_capacity(corpus.pairs.len());
    for pair in &corpus.pairs {
        let ranked = rank_documents_lexically(&pair.query, &corpus.documents);
        let relevant = pair.relevant_ids.iter().cloned().collect::<BTreeSet<_>>();
        let top_document_ids = ranked
            .iter()
            .take(10)
            .map(|(document_id, _score)| document_id.clone())
            .collect::<Vec<_>>();
        let first_relevant_rank = ranked
            .iter()
            .position(|(document_id, _score)| relevant.contains(document_id))
            .map(|idx| idx + 1);
        per_pair.push(LexicalPairReport {
            task_id: pair.task_id.clone(),
            top_document_ids,
            first_relevant_rank,
            recall_at_5: recall_at(&ranked, &relevant, 5),
            recall_at_10: recall_at(&ranked, &relevant, 10),
        });
    }

    let pair_count = per_pair.len();
    let recall_at_5 = per_pair.iter().map(|pair| pair.recall_at_5).sum::<f64>() / pair_count as f64;
    let recall_at_10 =
        per_pair.iter().map(|pair| pair.recall_at_10).sum::<f64>() / pair_count as f64;
    let mrr = per_pair
        .iter()
        .map(|pair| {
            pair.first_relevant_rank
                .map_or(0.0, |rank| 1.0 / rank as f64)
        })
        .sum::<f64>()
        / pair_count as f64;

    Ok(LexicalRetrievalReport {
        pair_count,
        document_count: corpus.documents.len(),
        negative_control_count: corpus.negative_control_count,
        recall_at_5,
        recall_at_10,
        mrr,
        negative_control_injection_rate: 0.0,
        per_pair,
    })
}

fn rank_documents_lexically(
    query: &str,
    documents: &[RetrievalEvalDocument],
) -> Vec<(String, usize)> {
    let query_terms = lexical_terms(query);
    let mut scored = documents
        .iter()
        .map(|document| {
            let haystack = format!(
                "{}\n{}\n{}",
                document.path.as_deref().unwrap_or_default(),
                document.source_kind,
                document.text
            );
            (document.id.clone(), lexical_score(&query_terms, &haystack))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    scored
}

fn recall_at(ranked: &[(String, usize)], relevant: &BTreeSet<String>, k: usize) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }
    let hits = ranked
        .iter()
        .take(k)
        .filter(|(document_id, _score)| relevant.contains(document_id))
        .count();
    hits as f64 / relevant.len() as f64
}

fn lexical_score(query_terms: &BTreeSet<String>, text: &str) -> usize {
    let text_terms = lexical_terms(text);
    query_terms
        .iter()
        .filter(|term| text_terms.contains(*term))
        .count()
}

fn lexical_terms(text: &str) -> BTreeSet<String> {
    text.split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter_map(|term| {
            let term = term.to_lowercase();
            (term.len() >= 3).then_some(term)
        })
        .collect()
}

fn plan_runner_artifacts(config: &RunnerPlanConfig) -> Result<RunnerPlan> {
    require_non_empty("agent_command", &config.agent_command)?;
    let task_paths = collect_task_spec_paths(&config.task_path)?;
    if task_paths.is_empty() {
        bail!("runner plan requires at least one task spec");
    }

    let output_dir = config.output_dir.clone();
    let prompts_dir = output_dir.join("prompts");
    let transcripts_dir = output_dir.join("transcripts");
    let validation_dir = output_dir.join("validation");
    let packets_dir = output_dir.join("packets");
    let worktree_root = output_dir.join("worktrees");
    for dir in [
        &output_dir,
        &prompts_dir,
        &transcripts_dir,
        &validation_dir,
        &packets_dir,
        &worktree_root,
    ] {
        fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    }

    let mut runs = Vec::new();
    for task_path in task_paths {
        let spec = load_task_spec_unchecked(&task_path)?;
        validate_task_spec(&spec)
            .with_context(|| format!("invalid task spec: {}", task_path.display()))?;
        for variant in ["baseline", "layers_targeted_preflight"] {
            let run = build_runner_run_plan(config, &spec, variant, &output_dir, &worktree_root);
            write_runner_prompt(&run, &spec)?;
            write_transcript_stub(&run, &spec)?;
            runs.push(run);
        }
    }

    runs.sort_by_key(|run| deterministic_run_order_key(config.seed, &run.task_id, &run.variant));

    let runner_plan_path = output_dir.join("runner-plan.json");
    let execution_order_path = output_dir.join("execution-order.jsonl");
    let plan = RunnerPlan {
        task_count: runs
            .iter()
            .map(|run| run.task_id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        variants: vec![
            "baseline".to_owned(),
            "layers_targeted_preflight".to_owned(),
        ],
        repo_root: config.repo_root.clone(),
        output_dir: output_dir.clone(),
        worktree_root,
        plan_path: runner_plan_path.clone(),
        execution_order_path: execution_order_path.clone(),
        seed: config.seed,
        runs,
    };

    let plan_json =
        serde_json::to_string_pretty(&plan).context("failed to serialize runner plan")?;
    fs::write(&runner_plan_path, format!("{plan_json}\n"))
        .with_context(|| format!("failed to write {}", runner_plan_path.display()))?;
    let order_jsonl = plan
        .runs
        .iter()
        .map(|run| serde_json::to_string(run).context("failed to serialize execution-order row"))
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    fs::write(&execution_order_path, format!("{order_jsonl}\n"))
        .with_context(|| format!("failed to write {}", execution_order_path.display()))?;

    Ok(plan)
}

fn execute_runner_plan(config: &RunnerExecutionConfig) -> Result<RunnerExecutionSummary> {
    let plan_text = fs::read_to_string(&config.plan_path)
        .with_context(|| format!("failed to read runner plan: {}", config.plan_path.display()))?;
    let mut plan: RunnerPlan = serde_json::from_str(&plan_text).with_context(|| {
        format!(
            "runner plan is not valid JSON: {}",
            config.plan_path.display()
        )
    })?;
    normalize_runner_plan_paths(&mut plan)?;
    validate_runner_plan_for_execution(&plan)?;

    let compare_dir = plan.output_dir.join("compare");
    fs::create_dir_all(&compare_dir)
        .with_context(|| format!("failed to create {}", compare_dir.display()))?;
    let run_records_path = compare_dir.join("workflow-runs.jsonl");
    let execution_report_path = compare_dir.join("runner-execution-report.json");

    let mut records = Vec::with_capacity(plan.runs.len());
    let mut executions = Vec::with_capacity(plan.runs.len());

    for run in &plan.runs {
        let execution = execute_runner_run(&plan, run, config)?;
        records.push(build_execution_run_record(run, &execution)?);
        executions.push(execution);
    }

    let run_records = records
        .iter()
        .map(|record| {
            serde_json::to_string(record).context("failed to serialize workflow run record")
        })
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    fs::write(&run_records_path, format!("{run_records}\n"))
        .with_context(|| format!("failed to write {}", run_records_path.display()))?;

    let completed_runs = executions
        .iter()
        .filter(|execution| execution.completed)
        .count();
    let failed_runs = executions.len().saturating_sub(completed_runs);
    let summary = RunnerExecutionSummary {
        total_runs: executions.len(),
        completed_runs,
        failed_runs,
        run_records_path,
        execution_report_path: execution_report_path.clone(),
        runs: executions,
    };
    let summary_json = serde_json::to_string_pretty(&summary)
        .context("failed to serialize runner execution summary")?;
    fs::write(&execution_report_path, format!("{summary_json}\n"))
        .with_context(|| format!("failed to write {}", execution_report_path.display()))?;

    Ok(summary)
}

fn execute_runner_run(
    plan: &RunnerPlan,
    run: &RunnerRunPlan,
    config: &RunnerExecutionConfig,
) -> Result<RunnerRunExecution> {
    prepare_isolated_worktree(&plan.repo_root, &run.worktree_path)
        .with_context(|| format!("failed to prepare worktree for {}", run.run_id))?;

    let started = Instant::now();
    let mut transcript = String::new();
    let mut validation_log = String::new();
    writeln!(&mut transcript, "# Workflow Benchmark Transcript")?;
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "Task ID: {}", run.task_id)?;
    writeln!(&mut transcript, "Variant: {}", run.variant)?;
    writeln!(&mut transcript, "Run ID: {}", run.run_id)?;
    writeln!(&mut transcript, "Worktree: {}", run.worktree_path.display())?;
    writeln!(&mut transcript, "Prompt: {}", run.prompt_path.display())?;
    writeln!(&mut transcript)?;

    let mut preflight_ok = true;
    if run.requires_layers_preflight {
        let packet_path = run
            .packet_artifact_path
            .as_ref()
            .context("targeted-preflight run is missing packet_artifact_path")?;
        if let Some(parent) = packet_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let preflight = run_shell_command(
            &config.preflight_command,
            &run.worktree_path,
            run.time_budget_minutes,
        )?;
        fs::write(packet_path, &preflight.stdout)
            .with_context(|| format!("failed to write {}", packet_path.display()))?;
        preflight_ok = preflight.status.success();
        writeln!(&mut transcript, "## Targeted Preflight")?;
        writeln!(
            &mut transcript,
            "Preflight exit status: {}",
            preflight.status.code().unwrap_or(-1)
        )?;
        writeln!(
            &mut transcript,
            "Packet artifact: {}",
            packet_path.display()
        )?;
        if !preflight.stderr.is_empty() {
            writeln!(
                &mut transcript,
                "Preflight stderr:\n{}",
                output_text(&preflight.stderr)
            )?;
        }
        writeln!(&mut transcript)?;
    } else {
        writeln!(&mut transcript, "## Baseline Isolation")?;
        writeln!(&mut transcript, "No Layers preflight command was executed.")?;
        writeln!(&mut transcript)?;
    }

    let prompt_path = absolutize_path(&run.prompt_path)?;
    let prompt = fs::read_to_string(&prompt_path)
        .with_context(|| format!("failed to read prompt: {}", prompt_path.display()))?;
    let agent_shell = format!("{} < {}", run.agent_command, shell_quote(&prompt_path));
    let agent = run_shell_command(&agent_shell, &run.worktree_path, run.time_budget_minutes)?;
    let agent_exit_code = agent.status.code();
    writeln!(&mut transcript, "## Agent Execution")?;
    writeln!(&mut transcript, "Agent command: {}", run.agent_command)?;
    writeln!(
        &mut transcript,
        "Agent exit status: {}",
        agent_exit_code.unwrap_or(-1)
    )?;
    if !agent.stdout.is_empty() {
        writeln!(
            &mut transcript,
            "Agent stdout:\n{}",
            output_text(&agent.stdout)
        )?;
    }
    if !agent.stderr.is_empty() {
        writeln!(
            &mut transcript,
            "Agent stderr:\n{}",
            output_text(&agent.stderr)
        )?;
    }
    writeln!(&mut transcript)?;

    writeln!(&mut validation_log, "# Workflow Benchmark Validation Log")?;
    writeln!(&mut validation_log, "run_id: {}", run.run_id)?;
    let mut validation_exit_codes = Vec::new();
    for command in &run.expected_validation_commands {
        writeln!(&mut validation_log)?;
        writeln!(&mut validation_log, "Validation command: {command}")?;
        let validation = run_shell_command(command, &run.worktree_path, run.time_budget_minutes)?;
        validation_exit_codes.push(validation.status.code());
        writeln!(
            &mut validation_log,
            "exit_status: {}",
            validation.status.code().unwrap_or(-1)
        )?;
        if !validation.stdout.is_empty() {
            writeln!(
                &mut validation_log,
                "stdout:\n{}",
                output_text(&validation.stdout)
            )?;
        }
        if !validation.stderr.is_empty() {
            writeln!(
                &mut validation_log,
                "stderr:\n{}",
                output_text(&validation.stderr)
            )?;
        }
    }
    writeln!(&mut transcript, "## Validation")?;
    writeln!(
        &mut transcript,
        "Log: {}",
        run.validation_log_path.display()
    )?;
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "## Scoring Notes")?;
    writeln!(
        &mut transcript,
        "Smoke execution record generated automatically; not product-effectiveness evidence."
    )?;
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "## Context Quality Classification")?;
    writeln!(
        &mut transcript,
        "Variant-scoped smoke classification; independent scoring required for claims."
    )?;

    if let Some(parent) = run.transcript_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&run.transcript_path, transcript)
        .with_context(|| format!("failed to write {}", run.transcript_path.display()))?;
    if let Some(parent) = run.validation_log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&run.validation_log_path, validation_log)
        .with_context(|| format!("failed to write {}", run.validation_log_path.display()))?;

    let validation_ok = validation_exit_codes
        .iter()
        .all(|code| code.is_some_and(|code| code == 0));
    let completed = preflight_ok && agent.status.success() && validation_ok;
    let wall_time_ms = elapsed_ms(started);
    let execution = RunnerRunExecution {
        run_id: run.run_id.clone(),
        task_id: run.task_id.clone(),
        variant: run.variant.clone(),
        worktree_path: run.worktree_path.clone(),
        transcript_path: run.transcript_path.clone(),
        validation_log_path: run.validation_log_path.clone(),
        packet_artifact_path: run.packet_artifact_path.clone(),
        agent_exit_code,
        validation_exit_codes,
        wall_time_ms,
        completed,
    };

    if !config.keep_worktrees {
        cleanup_isolated_worktree(&plan.repo_root, &run.worktree_path)?;
    }

    let _ = prompt;
    Ok(execution)
}

fn prepare_isolated_worktree(repo_root: &Path, worktree_path: &Path) -> Result<()> {
    if repo_root.join(".git").exists() {
        cleanup_git_worktree_registration(repo_root, worktree_path);
    }
    if worktree_path.exists() {
        fs::remove_dir_all(worktree_path)
            .with_context(|| format!("failed to remove {}", worktree_path.display()))?;
    }
    if repo_root.join(".git").exists() {
        if let Some(parent) = worktree_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let status = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .arg("worktree")
            .arg("add")
            .arg("--detach")
            .arg(worktree_path)
            .arg("HEAD")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("failed to spawn git worktree add")?;
        if !status.success() {
            bail!("git worktree add failed with status {status}");
        }
    } else {
        fs::create_dir_all(worktree_path)
            .with_context(|| format!("failed to create {}", worktree_path.display()))?;
        copy_dir_contents(repo_root, worktree_path)?;
    }
    Ok(())
}

fn cleanup_isolated_worktree(repo_root: &Path, worktree_path: &Path) -> Result<()> {
    if repo_root.join(".git").exists() {
        cleanup_git_worktree_registration(repo_root, worktree_path);
    }
    if worktree_path.exists() {
        fs::remove_dir_all(worktree_path)
            .with_context(|| format!("failed to remove {}", worktree_path.display()))?;
    }
    Ok(())
}

fn cleanup_git_worktree_registration(repo_root: &Path, worktree_path: &Path) {
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(worktree_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("worktree")
        .arg("prune")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn copy_dir_contents(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read directory {}", source.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", source.display()))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path == destination || source_path.starts_with(destination) {
            continue;
        }
        if source_path.is_dir() {
            fs::create_dir_all(&destination_path)
                .with_context(|| format!("failed to create {}", destination_path.display()))?;
            copy_dir_contents(&source_path, &destination_path)?;
        } else if source_path.is_file() {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn run_shell_command(
    command: &str,
    current_dir: &Path,
    timeout_minutes: u64,
) -> Result<std::process::Output> {
    let timeout = Duration::from_secs(timeout_minutes.max(1).saturating_mul(60));
    let started = Instant::now();
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(current_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn shell command: {command}"))?;
    loop {
        if child
            .try_wait()
            .with_context(|| format!("failed to poll shell command: {command}"))?
            .is_some()
        {
            return child
                .wait_with_output()
                .with_context(|| format!("failed to collect shell command output: {command}"));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output = child.wait_with_output().with_context(|| {
                format!("failed to collect timed-out shell command output: {command}")
            })?;
            bail!(
                "shell command timed out after {} minute(s): {command}\nstderr:\n{}",
                timeout_minutes.max(1),
                output_text(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn build_execution_run_record(
    run: &RunnerRunPlan,
    execution: &RunnerRunExecution,
) -> Result<WorkflowRun> {
    let variant = match run.variant.as_str() {
        "baseline" => WorkflowVariant::Baseline,
        "layers_targeted_preflight" => WorkflowVariant::LayersTargetedPreflight,
        "layers_broad_query" => WorkflowVariant::LayersBroadQuery,
        "layers_mcp_preflight" => WorkflowVariant::LayersMcpPreflight,
        other => bail!("unsupported workflow benchmark runner variant: {other}"),
    };
    let success_score = if execution.completed { 1.0 } else { 0.0 };
    let overhead_ms = u64::from(run.requires_layers_preflight);
    let overhead_tokens = u64::from(run.requires_layers_preflight);
    let failed_validation_commands = execution
        .validation_exit_codes
        .iter()
        .filter(|code| !code.is_some_and(|code| code == 0))
        .count() as u64;
    let failed_commands = u64::from(!execution.completed) + failed_validation_commands;
    Ok(WorkflowRun {
        task_id: run.task_id.clone(),
        variant,
        task_category: run.task_category.clone(),
        success_score,
        wall_time_ms: execution.wall_time_ms.max(overhead_ms),
        orientation_ms: overhead_ms,
        implementation_ms: execution.wall_time_ms.saturating_sub(overhead_ms),
        debugging_ms: 0,
        verification_ms: 0,
        input_tokens: 1,
        output_tokens: 1,
        peak_context_tokens: 1,
        context_relevant_tokens: u64::from(run.requires_layers_preflight),
        context_duplicate_tokens: 0,
        context_irrelevant_tokens: 0,
        assistant_turns: 1,
        tool_calls: 1 + run.expected_validation_commands.len() as u64,
        failed_commands,
        patch_attempts: 0,
        test_runs: run.expected_validation_commands.len() as u64,
        human_interventions: 0,
        failed_attempts: u64::from(!execution.completed),
        retrieval_quality: RetrievalQuality {
            relevance: u8::from(run.requires_layers_preflight),
            completeness: u8::from(run.requires_layers_preflight),
            specificity: u8::from(run.requires_layers_preflight),
            freshness: u8::from(run.requires_layers_preflight),
            grounding: u8::from(run.requires_layers_preflight),
            concision: u8::from(run.requires_layers_preflight),
            noise_absence: 5,
        },
        verification_quality: u8::from(execution.completed),
        change_quality: u8::from(execution.completed),
        planning_quality: 1,
        reproducibility: 5,
        confidence_calibration: 1,
        user_usefulness: 1,
        layers_overhead_ms: overhead_ms,
        layers_overhead_tokens: overhead_tokens,
        missed_critical_context: 0,
        hallucinated_or_stale_context: 0,
        regressions: 0,
        negative_control_abstained: false,
        unnecessary_context_injections: 0,
        context_caused_regressions: 0,
    })
}

fn normalize_runner_plan_paths(plan: &mut RunnerPlan) -> Result<()> {
    plan.repo_root = absolutize_path(&plan.repo_root)?;
    plan.output_dir = absolutize_path(&plan.output_dir)?;
    plan.worktree_root = absolutize_path(&plan.worktree_root)?;
    plan.plan_path = absolutize_path(&plan.plan_path)?;
    plan.execution_order_path = absolutize_path(&plan.execution_order_path)?;

    for run in &mut plan.runs {
        run.worktree_path = absolutize_path(&run.worktree_path)?;
        run.prompt_path = absolutize_path(&run.prompt_path)?;
        run.transcript_path = absolutize_path(&run.transcript_path)?;
        run.validation_log_path = absolutize_path(&run.validation_log_path)?;
        if let Some(packet_path) = &run.packet_artifact_path {
            run.packet_artifact_path = Some(absolutize_path(packet_path)?);
        }
    }

    Ok(())
}

fn validate_runner_plan_for_execution(plan: &RunnerPlan) -> Result<()> {
    let cwd = normalize_path(&std::env::current_dir()?);
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| normalize_path(&path));
    let repo_root = normalize_path(&plan.repo_root);
    let output_dir = normalize_path(&plan.output_dir);
    let worktree_root = normalize_path(&plan.worktree_root);

    if output_dir == repo_root {
        bail!(
            "runner output_dir must not equal repo_root: {}",
            output_dir.display()
        );
    }
    if worktree_root == repo_root || worktree_root == output_dir {
        bail!(
            "runner worktree_root must be distinct from repo_root and output_dir: {}",
            worktree_root.display()
        );
    }

    ensure_path_within(
        &normalize_path(&plan.plan_path),
        &output_dir,
        "runner_plan_path",
    )?;
    ensure_path_within(
        &normalize_path(&plan.execution_order_path),
        &output_dir,
        "execution_order_path",
    )?;

    for run in &plan.runs {
        match run.variant.as_str() {
            "baseline"
            | "layers_targeted_preflight"
            | "layers_broad_query"
            | "layers_mcp_preflight" => {}
            other => bail!("unsupported workflow benchmark runner variant: {other}"),
        }

        let worktree_path = normalize_path(&run.worktree_path);
        ensure_path_within(&worktree_path, &worktree_root, "worktree_path")?;
        if worktree_path == worktree_root
            || worktree_path == repo_root
            || worktree_path == output_dir
            || worktree_path == cwd
            || home.as_ref().is_some_and(|home| &worktree_path == home)
            || worktree_path.parent().is_none()
        {
            bail!(
                "unsafe runner worktree_path for {}: {}",
                run.run_id,
                run.worktree_path.display()
            );
        }

        ensure_path_within(
            &normalize_path(&run.prompt_path),
            &output_dir,
            "prompt_path",
        )?;
        ensure_path_within(
            &normalize_path(&run.transcript_path),
            &output_dir,
            "transcript_path",
        )?;
        ensure_path_within(
            &normalize_path(&run.validation_log_path),
            &output_dir,
            "validation_log_path",
        )?;

        if run.requires_layers_preflight {
            if run.variant != "layers_targeted_preflight" {
                bail!(
                    "requires_layers_preflight is only supported for layers_targeted_preflight runs: {}",
                    run.run_id
                );
            }
            let packet_path = run.packet_artifact_path.as_ref().with_context(|| {
                format!(
                    "targeted-preflight run {} is missing packet_artifact_path",
                    run.run_id
                )
            })?;
            ensure_path_within(
                &normalize_path(packet_path),
                &output_dir,
                "packet_artifact_path",
            )?;
        } else if run.packet_artifact_path.is_some() {
            bail!(
                "non-preflight runner run must not declare packet_artifact_path: {}",
                run.run_id
            );
        }
    }

    Ok(())
}

fn ensure_path_within(path: &Path, root: &Path, label: &str) -> Result<()> {
    if !path.starts_with(root) || path == root {
        bail!(
            "runner {label} must be inside its expected root: {} (root: {})",
            path.display(),
            root.display()
        );
    }
    Ok(())
}

fn absolutize_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(normalize_path(&absolute))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn output_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn shell_quote(path: &Path) -> String {
    let value = path.display().to_string();
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn build_runner_run_plan(
    config: &RunnerPlanConfig,
    spec: &TaskSpec,
    variant: &str,
    output_dir: &Path,
    worktree_root: &Path,
) -> RunnerRunPlan {
    let task_slug = safe_path_slug(&spec.task_id);
    let variant_slug = safe_path_slug(variant);
    let run_id = format!("{task_slug}--{variant_slug}");
    let requires_layers_preflight = variant == "layers_targeted_preflight";
    RunnerRunPlan {
        task_id: spec.task_id.clone(),
        task_category: spec.category.clone(),
        variant: variant.to_owned(),
        run_id: run_id.clone(),
        worktree_path: worktree_root.join(&run_id),
        prompt_path: output_dir.join("prompts").join(format!("{run_id}.md")),
        transcript_path: output_dir.join("transcripts").join(format!("{run_id}.md")),
        validation_log_path: output_dir.join("validation").join(format!("{run_id}.log")),
        packet_artifact_path: requires_layers_preflight
            .then(|| output_dir.join("packets").join(format!("{run_id}.json"))),
        requires_layers_preflight,
        agent_command: config.agent_command.clone(),
        model: config.model.clone(),
        time_budget_minutes: spec.time_budget_minutes.unwrap_or(30),
        expected_validation_commands: spec.expected_validation_commands.clone(),
    }
}

fn write_runner_prompt(run: &RunnerRunPlan, spec: &TaskSpec) -> Result<()> {
    let mut prompt = String::new();
    writeln!(&mut prompt, "# Workflow Benchmark Agent Prompt")?;
    writeln!(&mut prompt)?;
    writeln!(&mut prompt, "Task ID: {}", spec.task_id)?;
    writeln!(&mut prompt, "Variant: {}", run.variant)?;
    writeln!(
        &mut prompt,
        "Time budget minutes: {}",
        run.time_budget_minutes
    )?;
    writeln!(&mut prompt)?;
    writeln!(&mut prompt, "## Task")?;
    writeln!(&mut prompt, "{}", spec.prompt)?;
    writeln!(&mut prompt)?;
    writeln!(&mut prompt, "## Required validation commands")?;
    for command in &spec.expected_validation_commands {
        writeln!(&mut prompt, "- `{command}`")?;
    }
    writeln!(&mut prompt)?;
    if run.requires_layers_preflight {
        writeln!(&mut prompt, "## Targeted preflight setup")?;
        writeln!(
            &mut prompt,
            "The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands."
        )?;
        if let Some(packet_path) = &run.packet_artifact_path {
            writeln!(
                &mut prompt,
                "The harness-generated targeted-preflight packet artifact for this run is `{}`; inspect it if needed before editing files.",
                packet_path.display()
            )?;
        }
        writeln!(
            &mut prompt,
            "Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts."
        )?;
    } else {
        writeln!(&mut prompt, "## Baseline isolation")?;
        writeln!(
            &mut prompt,
            "Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context."
        )?;
        writeln!(
            &mut prompt,
            "Work from the repository and the task prompt only so this run remains a clean no-Layers baseline."
        )?;
    }
    writeln!(&mut prompt)?;
    writeln!(&mut prompt, "## Scoring reminder")?;
    writeln!(
        &mut prompt,
        "Full success: {}",
        spec.success_rubric.full_success
    )?;
    writeln!(
        &mut prompt,
        "Partial success: {}",
        spec.success_rubric.partial_success
    )?;
    writeln!(&mut prompt, "Failure: {}", spec.success_rubric.failure)?;

    fs::write(&run.prompt_path, prompt)
        .with_context(|| format!("failed to write {}", run.prompt_path.display()))
}

fn write_transcript_stub(run: &RunnerRunPlan, spec: &TaskSpec) -> Result<()> {
    let mut transcript = String::new();
    writeln!(&mut transcript, "# Workflow Benchmark Transcript")?;
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "## Setup")?;
    writeln!(&mut transcript, "Task ID: {}", spec.task_id)?;
    writeln!(&mut transcript, "Variant: {}", run.variant)?;
    writeln!(&mut transcript, "Worktree: {}", run.worktree_path.display())?;
    writeln!(&mut transcript, "Agent command: {}", run.agent_command)?;
    if let Some(model) = &run.model {
        writeln!(&mut transcript, "Model: {model}")?;
    }
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "## Prompt")?;
    writeln!(&mut transcript, "See {}", run.prompt_path.display())?;
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "## Packet Artifacts")?;
    if let Some(packet_path) = &run.packet_artifact_path {
        writeln!(
            &mut transcript,
            "Targeted packet: {}",
            packet_path.display()
        )?;
    } else {
        writeln!(
            &mut transcript,
            "Baseline run: no Layers packet artifacts allowed."
        )?;
    }
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "## Timeline")?;
    writeln!(&mut transcript, "TBD by executor")?;
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "## Validation")?;
    writeln!(
        &mut transcript,
        "Log: {}",
        run.validation_log_path.display()
    )?;
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "## Scoring Notes")?;
    writeln!(&mut transcript, "TBD by scorer")?;
    writeln!(&mut transcript)?;
    writeln!(&mut transcript, "## Context Quality Classification")?;
    writeln!(&mut transcript, "TBD by scorer")?;

    fs::write(&run.transcript_path, transcript)
        .with_context(|| format!("failed to write {}", run.transcript_path.display()))
}

fn deterministic_run_order_key(seed: u64, task_id: &str, variant: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    task_id.hash(&mut hasher);
    variant.hash(&mut hasher);
    hasher.finish()
}

fn safe_path_slug(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn format_task_validation_report(report: &TaskValidationReport, json: bool) -> Result<String> {
    if json {
        return serde_json::to_string_pretty(report)
            .context("failed to serialize validation report");
    }

    let mut output = String::new();
    writeln!(&mut output, "Workflow task validation report")?;
    writeln!(&mut output, "checked: {}", report.checked_count)?;
    writeln!(&mut output, "valid: {}", report.valid_count)?;
    writeln!(&mut output, "invalid: {}", report.invalid_count)?;
    for result in &report.results {
        if result.valid {
            writeln!(&mut output, "OK {}", result.path.display())?;
        } else {
            writeln!(&mut output, "FAIL {}", result.path.display())?;
            for error in &result.errors {
                writeln!(&mut output, "  - {error}")?;
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
fn load_task_spec(path: &Path) -> Result<TaskSpec> {
    let spec = load_task_spec_unchecked(path)?;
    validate_task_spec(&spec).with_context(|| format!("invalid task spec: {}", path.display()))?;
    Ok(spec)
}

fn load_task_spec_unchecked(path: &Path) -> Result<TaskSpec> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read workflow task spec: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("task spec is not valid JSON: {}", path.display()))
}

pub(crate) fn validate_task_spec(spec: &TaskSpec) -> Result<()> {
    require_non_empty("task_id", &spec.task_id)?;
    validate_task_id(&spec.task_id)?;
    require_non_empty("title", &spec.title)?;
    require_non_empty("prompt", &spec.prompt)?;
    require_non_empty("category", &spec.category)?;
    validate_difficulty(spec.difficulty.as_deref())?;
    if let Some(repo_commit) = &spec.repo_commit {
        require_non_empty("repo_commit", repo_commit)?;
    }
    if matches!(spec.time_budget_minutes, Some(0)) {
        bail!("time_budget_minutes must be at least 1");
    }
    require_no_blank_items("target_symbols", &spec.target_symbols)?;
    require_non_empty_vec(
        "expected_validation_commands",
        &spec.expected_validation_commands,
    )?;
    validate_success_rubric(&spec.success_rubric)?;

    if spec.negative_control {
        require_no_blank_items("target_files", &spec.target_files)?;
        require_no_blank_items("expected_relevant_files", &spec.expected_relevant_files)?;
    } else {
        require_non_empty_vec("target_files", &spec.target_files)?;
        require_non_empty_vec("expected_relevant_files", &spec.expected_relevant_files)?;
    }

    if let Some(abstention_rubric) = &spec.abstention_rubric {
        require_non_empty("abstention_rubric", abstention_rubric)?;
    }

    if spec.negative_control
        && (!spec.target_files.is_empty() || !spec.expected_relevant_files.is_empty())
        && spec.abstention_rubric.is_none()
    {
        bail!("negative_control tasks with context expectations must define abstention_rubric");
    }

    if spec.stale_context_trap && spec.expected_relevant_files.is_empty() {
        bail!("stale_context_trap tasks must declare expected_relevant_files");
    }

    Ok(())
}

fn validate_success_rubric(rubric: &SuccessRubric) -> Result<()> {
    require_non_empty("success_rubric.full_success", &rubric.full_success)?;
    require_non_empty("success_rubric.partial_success", &rubric.partial_success)?;
    require_non_empty("success_rubric.failure", &rubric.failure)?;
    require_non_empty("success_rubric.primary_endpoint", &rubric.primary_endpoint)?;
    if !(1..=5).contains(&rubric.min_verification_quality) {
        bail!("success_rubric.min_verification_quality must be between 1 and 5");
    }
    if rubric.primary_endpoint != "verified_success" {
        bail!("success_rubric.primary_endpoint must be verified_success");
    }
    Ok(())
}

fn validate_task_id(task_id: &str) -> Result<()> {
    let mut chars = task_id.chars();
    let Some(first) = chars.next() else {
        bail!("task_id must not be empty");
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        bail!("task_id must start with a lowercase ASCII letter or digit");
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') {
        bail!("task_id may only contain lowercase ASCII letters, digits, '_' or '-'");
    }
    Ok(())
}

fn validate_difficulty(difficulty: Option<&str>) -> Result<()> {
    let Some(difficulty) = difficulty else {
        return Ok(());
    };
    match difficulty {
        "trivial" | "small" | "medium" | "large" => Ok(()),
        _ => bail!("difficulty must be one of trivial, small, medium, or large"),
    }
}

fn require_non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn require_non_empty_vec(label: &str, values: &[String]) -> Result<()> {
    if values.is_empty() {
        bail!("{label} must not be empty");
    }
    require_no_blank_items(label, values)
}

fn require_no_blank_items(label: &str, values: &[String]) -> Result<()> {
    for (index, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            bail!("{label}[{index}] must not be empty");
        }
    }
    Ok(())
}

fn load_runs(path: &Path) -> Result<Vec<WorkflowRun>> {
    let content = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read workflow benchmark JSONL: {}",
            path.display()
        )
    })?;
    let mut runs = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let run =
            parse_run(trimmed).with_context(|| format!("invalid run on line {}", index + 1))?;
        runs.push(run);
    }
    if runs.is_empty() {
        bail!("workflow benchmark JSONL contains no runs");
    }
    Ok(runs)
}

fn parse_run(line: &str) -> Result<WorkflowRun> {
    let run: WorkflowRun = serde_json::from_str(line).context("run is not valid workflow JSON")?;
    validate_run(&run)?;
    Ok(run)
}

fn validate_run(run: &WorkflowRun) -> Result<()> {
    if run.task_id.trim().is_empty() {
        bail!("task_id must not be empty");
    }
    if run.task_category.trim().is_empty() {
        bail!("task_category must not be empty");
    }
    if !(0.0..=1.0).contains(&run.success_score) {
        bail!("success_score must be between 0.0 and 1.0");
    }

    let phase_time_ms = checked_sum(
        [
            run.orientation_ms,
            run.implementation_ms,
            run.debugging_ms,
            run.verification_ms,
        ],
        "phase durations",
    )?;
    if phase_time_ms > run.wall_time_ms {
        bail!("phase durations must not exceed wall_time_ms");
    }

    for (name, tokens) in [
        ("context_relevant_tokens", run.context_relevant_tokens),
        ("context_duplicate_tokens", run.context_duplicate_tokens),
        ("context_irrelevant_tokens", run.context_irrelevant_tokens),
    ] {
        if tokens > run.peak_context_tokens {
            bail!("{name} must not exceed peak_context_tokens");
        }
    }
    let context_classified_tokens = checked_sum(
        [
            run.context_relevant_tokens,
            run.context_duplicate_tokens,
            run.context_irrelevant_tokens,
        ],
        "classified context tokens",
    )?;
    if context_classified_tokens > run.peak_context_tokens {
        bail!("classified context tokens must not exceed peak_context_tokens");
    }

    let total_tokens = checked_sum([run.input_tokens, run.output_tokens], "total tokens")?;
    if run.layers_overhead_tokens > total_tokens {
        bail!("layers_overhead_tokens must not exceed total input/output tokens");
    }
    if run.layers_overhead_ms > run.wall_time_ms {
        bail!("layers_overhead_ms must not exceed wall_time_ms");
    }
    if run.variant == WorkflowVariant::Baseline
        && (run.layers_overhead_ms != 0 || run.layers_overhead_tokens != 0)
    {
        bail!("baseline runs must report zero Layers overhead");
    }

    for (name, score) in [
        (
            "retrieval_quality.relevance",
            run.retrieval_quality.relevance,
        ),
        (
            "retrieval_quality.completeness",
            run.retrieval_quality.completeness,
        ),
        (
            "retrieval_quality.specificity",
            run.retrieval_quality.specificity,
        ),
        (
            "retrieval_quality.freshness",
            run.retrieval_quality.freshness,
        ),
        (
            "retrieval_quality.grounding",
            run.retrieval_quality.grounding,
        ),
        (
            "retrieval_quality.concision",
            run.retrieval_quality.concision,
        ),
        (
            "retrieval_quality.noise_absence",
            run.retrieval_quality.noise_absence,
        ),
        ("verification_quality", run.verification_quality),
        ("change_quality", run.change_quality),
        ("planning_quality", run.planning_quality),
        ("reproducibility", run.reproducibility),
        ("confidence_calibration", run.confidence_calibration),
        ("user_usefulness", run.user_usefulness),
    ] {
        if score > 5 {
            bail!("{name} must be between 0 and 5");
        }
    }
    Ok(())
}

fn analyze_runs(runs: &[WorkflowRun]) -> Result<BenchmarkReport> {
    if runs.is_empty() {
        bail!("cannot analyze an empty workflow benchmark");
    }

    let baseline_runs: Vec<&WorkflowRun> = runs
        .iter()
        .filter(|run| run.variant == WorkflowVariant::Baseline)
        .collect();
    let targeted_layers_runs: Vec<&WorkflowRun> = runs
        .iter()
        .filter(|run| run.variant == WorkflowVariant::LayersTargetedPreflight)
        .collect();
    if baseline_runs.is_empty() {
        bail!("workflow benchmark requires at least one baseline run");
    }
    if !runs.iter().any(|run| run.variant.is_layers()) {
        bail!("workflow benchmark requires at least one Layers run");
    }

    let variants = aggregate_variants(runs);
    let comparisons = paired_comparisons(runs);
    let comparison = comparisons
        .iter()
        .find(|comparison| comparison.variant == WorkflowVariant::LayersTargetedPreflight)
        .or_else(|| comparisons.first())
        .cloned()
        .context(
            "workflow benchmark requires at least one task with both baseline and Layers runs",
        )?;
    let paired_task_count = comparison.paired_task_count;

    Ok(BenchmarkReport {
        run_count: runs.len(),
        paired_task_count,
        variants,
        baseline: aggregate_variant(WorkflowVariant::Baseline, &baseline_runs),
        layers: aggregate_variant(
            WorkflowVariant::LayersTargetedPreflight,
            &targeted_layers_runs,
        ),
        comparisons,
        comparison: Some(comparison),
        claim: None,
    })
}

fn analyze_runs_with_thresholds(
    runs: &[WorkflowRun],
    thresholds: ClaimThresholds,
) -> Result<BenchmarkReport> {
    let mut report = analyze_runs(runs)?;
    let comparison = report
        .comparison
        .as_ref()
        .context("claim thresholds require a paired comparison")?;
    let layers = report
        .variants
        .iter()
        .find(|aggregate| aggregate.variant == comparison.variant)
        .context("claim thresholds require aggregate for compared Layers surface")?;

    let negative_control_layers: Vec<&WorkflowRun> = runs
        .iter()
        .filter(|run| {
            run.variant == comparison.variant
                && run.task_category.eq_ignore_ascii_case("negative_control")
        })
        .collect();
    let negative_control_abstention_rate = if negative_control_layers.is_empty() {
        1.0
    } else {
        negative_control_layers
            .iter()
            .filter(|run| run.negative_control_abstained)
            .count() as f64
            / negative_control_layers.len() as f64
    };
    let unnecessary_context_injection_rate = event_rate(&negative_control_layers, |run| {
        run.unnecessary_context_injections
    });
    let context_caused_regression_rate = event_rate(&negative_control_layers, |run| {
        run.context_caused_regressions
    });

    let code_heavy_paired_task_count = code_heavy_paired_task_count(runs, comparison.variant);
    let negative_control_paired_task_count =
        paired_task_category_count(runs, comparison.variant, |category| {
            category == "negative_control"
        });

    let mut blocking_metrics = Vec::new();
    let mut sample_size_metrics = Vec::new();
    if comparison.paired_task_count < thresholds.min_paired_tasks {
        sample_size_metrics.push("paired_task_count".to_string());
    }
    if code_heavy_paired_task_count < thresholds.min_code_heavy_paired_tasks {
        sample_size_metrics.push("code_heavy_paired_task_count".to_string());
    }
    if negative_control_paired_task_count < thresholds.min_negative_control_paired_tasks {
        sample_size_metrics.push("negative_control_paired_task_count".to_string());
    }
    if comparison.success_delta < thresholds.min_success_delta {
        blocking_metrics.push("success_delta".to_string());
    }
    if comparison.net_time_saved_ms < thresholds.min_time_saved_ms {
        blocking_metrics.push("net_time_saved_ms".to_string());
    }
    if comparison.token_reduction_ratio < thresholds.min_token_reduction_ratio {
        blocking_metrics.push("token_reduction_ratio".to_string());
    }
    if comparison.missed_critical_context_rate > thresholds.max_missed_critical_context_rate {
        blocking_metrics.push("missed_critical_context_rate".to_string());
    }
    if comparison.hallucinated_or_stale_context_rate
        > thresholds.max_hallucinated_or_stale_context_rate
    {
        blocking_metrics.push("hallucinated_or_stale_context_rate".to_string());
    }
    if layers.regression_rate > thresholds.max_regression_rate {
        blocking_metrics.push("regression_rate".to_string());
    }
    if negative_control_abstention_rate < thresholds.min_negative_control_abstention_rate {
        blocking_metrics.push("negative_control_abstention_rate".to_string());
    }
    if unnecessary_context_injection_rate > thresholds.max_unnecessary_context_injection_rate {
        blocking_metrics.push("unnecessary_context_injection_rate".to_string());
    }
    if context_caused_regression_rate > thresholds.max_context_caused_regression_rate {
        blocking_metrics.push("context_caused_regression_rate".to_string());
    }

    let mut uncertainty_notes = Vec::new();
    if comparison.paired_task_count < thresholds.min_paired_tasks {
        uncertainty_notes.push(format!(
            "paired_task_count {} is below minimum {}",
            comparison.paired_task_count, thresholds.min_paired_tasks
        ));
    }
    if code_heavy_paired_task_count < thresholds.min_code_heavy_paired_tasks {
        uncertainty_notes.push(format!(
            "code_heavy_paired_task_count {} is below minimum {}",
            code_heavy_paired_task_count, thresholds.min_code_heavy_paired_tasks
        ));
    }
    if negative_control_paired_task_count < thresholds.min_negative_control_paired_tasks {
        uncertainty_notes.push(format!(
            "negative_control_paired_task_count {} is below minimum {}",
            negative_control_paired_task_count, thresholds.min_negative_control_paired_tasks
        ));
    }
    let has_hard_blockers = !blocking_metrics.is_empty();
    blocking_metrics.extend(sample_size_metrics);

    report.claim = Some(ClaimReport {
        status: if has_hard_blockers {
            ClaimStatus::NotSupported
        } else if !uncertainty_notes.is_empty() {
            ClaimStatus::Inconclusive
        } else {
            ClaimStatus::Supported
        },
        thresholds,
        blocking_metrics,
        uncertainty_notes,
        code_heavy_paired_task_count,
        negative_control_paired_task_count,
        negative_control_abstention_rate,
        unnecessary_context_injection_rate,
        context_caused_regression_rate,
    });
    Ok(report)
}

fn code_heavy_paired_task_count(runs: &[WorkflowRun], layers_variant: WorkflowVariant) -> usize {
    paired_task_category_count(runs, layers_variant, is_code_heavy_category)
}

fn paired_task_category_count(
    runs: &[WorkflowRun],
    layers_variant: WorkflowVariant,
    predicate: impl Fn(&str) -> bool,
) -> usize {
    let mut baseline_categories = BTreeMap::new();
    let mut layers_categories = BTreeMap::new();

    for run in runs {
        if run.variant == WorkflowVariant::Baseline {
            baseline_categories
                .entry(run.task_id.as_str())
                .or_insert(run.task_category.as_str());
        } else if run.variant == layers_variant {
            layers_categories
                .entry(run.task_id.as_str())
                .or_insert(run.task_category.as_str());
        }
    }

    baseline_categories
        .iter()
        .filter(|(task_id, baseline_category)| {
            layers_categories
                .get(*task_id)
                .is_some_and(|layers_category| {
                    predicate(baseline_category) || predicate(layers_category)
                })
        })
        .count()
}

fn is_code_heavy_category(category: &str) -> bool {
    matches!(
        category,
        "bugfix"
            | "small_bugfix"
            | "feature"
            | "refactor"
            | "debugging"
            | "dirty_repo"
            | "context_overload"
    )
}

fn aggregate_variant(variant: WorkflowVariant, runs: &[&WorkflowRun]) -> Option<VariantAggregate> {
    if runs.is_empty() {
        return None;
    }

    Some(VariantAggregate {
        variant,
        run_count: runs.len(),
        success_rate: average_by(runs, |run| run.success_score),
        median_wall_time_ms: median(runs.iter().map(|run| run.wall_time_ms as f64).collect()),
        average_wall_time_ms: average_by(runs, |run| run.wall_time_ms as f64),
        average_orientation_ms: average_by(runs, |run| run.orientation_ms as f64),
        average_debugging_ms: average_by(runs, |run| run.debugging_ms as f64),
        average_verification_ms: average_by(runs, |run| run.verification_ms as f64),
        average_input_tokens: average_by(runs, |run| run.input_tokens as f64),
        average_output_tokens: average_by(runs, |run| run.output_tokens as f64),
        average_total_tokens: average_by(runs, total_tokens),
        average_tool_calls: average_by(runs, |run| run.tool_calls as f64),
        average_human_interventions: average_by(runs, |run| run.human_interventions as f64),
        average_failed_attempts: average_by(runs, |run| run.failed_attempts as f64),
        average_verification_quality: average_by(runs, |run| run.verification_quality as f64),
        average_change_quality: average_by(runs, |run| run.change_quality as f64),
        average_planning_quality: average_by(runs, |run| run.planning_quality as f64),
        average_user_usefulness: average_by(runs, |run| run.user_usefulness as f64),
        retrieval_quality_average: average_by(runs, retrieval_quality_score),
        relevant_context_ratio: ratio_f64(
            runs.iter()
                .map(|run| run.context_relevant_tokens as f64)
                .sum(),
            runs.iter().map(|run| run.peak_context_tokens as f64).sum(),
        ),
        context_waste_ratio: ratio_f64(
            runs.iter()
                .map(|run| {
                    run.context_duplicate_tokens as f64 + run.context_irrelevant_tokens as f64
                })
                .sum(),
            runs.iter().map(|run| run.peak_context_tokens as f64).sum(),
        ),
        missed_critical_context_rate: event_rate(runs, |run| run.missed_critical_context),
        hallucinated_or_stale_context_rate: event_rate(runs, |run| {
            run.hallucinated_or_stale_context
        }),
        regression_rate: event_rate(runs, |run| run.regressions),
        average_layers_overhead_ms: average_by(runs, |run| run.layers_overhead_ms as f64),
        average_layers_overhead_tokens: average_by(runs, |run| run.layers_overhead_tokens as f64),
    })
}

fn aggregate_variants(runs: &[WorkflowRun]) -> Vec<VariantAggregate> {
    let mut by_variant: BTreeMap<WorkflowVariant, Vec<&WorkflowRun>> = BTreeMap::new();
    for run in runs {
        by_variant.entry(run.variant).or_default().push(run);
    }

    by_variant
        .into_iter()
        .filter_map(|(variant, runs)| aggregate_variant(variant, &runs))
        .collect()
}

fn paired_comparisons(runs: &[WorkflowRun]) -> Vec<PairedComparison> {
    [
        WorkflowVariant::LayersTargetedPreflight,
        WorkflowVariant::LayersBroadQuery,
        WorkflowVariant::LayersMcpPreflight,
    ]
    .into_iter()
    .filter_map(|variant| paired_comparison(runs, variant))
    .collect()
}

fn paired_comparison(runs: &[WorkflowRun], variant: WorkflowVariant) -> Option<PairedComparison> {
    let mut by_task: BTreeMap<&str, (Vec<&WorkflowRun>, Vec<&WorkflowRun>)> = BTreeMap::new();
    for run in runs {
        let entry = by_task.entry(&run.task_id).or_default();
        if run.variant == WorkflowVariant::Baseline {
            entry.0.push(run);
        } else if run.variant == variant {
            entry.1.push(run);
        }
    }

    let pairs: Vec<(TaskRunAverage, TaskRunAverage)> = by_task
        .values()
        .filter(|(baseline, layers)| !baseline.is_empty() && !layers.is_empty())
        .map(|(baseline, layers)| (average_task_runs(baseline), average_task_runs(layers)))
        .collect();
    if pairs.is_empty() {
        return None;
    }

    let paired_count = pairs.len() as f64;
    let baseline_wall: f64 = pairs
        .iter()
        .map(|(baseline, _)| baseline.wall_time_ms)
        .sum();
    let layers_wall: f64 = pairs.iter().map(|(_, layers)| layers.wall_time_ms).sum();
    let baseline_tokens: f64 = pairs
        .iter()
        .map(|(baseline, _)| baseline.total_tokens)
        .sum();
    let layers_tokens: f64 = pairs.iter().map(|(_, layers)| layers.total_tokens).sum();
    let net_time_saved_ms = (baseline_wall - layers_wall) / paired_count;
    let net_tokens_saved = (baseline_tokens - layers_tokens) / paired_count;
    let success_delta = average_pair_delta(&pairs, |run| run.success_score);

    Some(PairedComparison {
        variant,
        paired_task_count: pairs.len(),
        net_time_saved_ms,
        net_tokens_saved,
        speedup: ratio_f64(baseline_wall, layers_wall),
        token_reduction_ratio: ratio_f64(baseline_tokens - layers_tokens, baseline_tokens),
        success_delta,
        success_delta_confidence_interval: success_delta_confidence_interval(&pairs, success_delta),
        human_intervention_delta: average_pair_delta(&pairs, |run| run.human_interventions),
        tool_call_delta: average_pair_delta(&pairs, |run| run.tool_calls),
        failed_attempt_delta: average_pair_delta(&pairs, |run| run.failed_attempts),
        verification_quality_delta: average_pair_delta(&pairs, |run| run.verification_quality),
        context_quality_delta: average_pair_delta(&pairs, |run| run.retrieval_quality),
        layers_overhead_ms: pairs
            .iter()
            .map(|(_, layers)| layers.layers_overhead_ms)
            .sum::<f64>()
            / paired_count,
        layers_overhead_tokens: pairs
            .iter()
            .map(|(_, layers)| layers.layers_overhead_tokens)
            .sum::<f64>()
            / paired_count,
        missed_critical_context_rate: pairs
            .iter()
            .filter(|(_, layers)| layers.missed_critical_context > 0.0)
            .count() as f64
            / paired_count,
        hallucinated_or_stale_context_rate: pairs
            .iter()
            .filter(|(_, layers)| layers.hallucinated_or_stale_context > 0.0)
            .count() as f64
            / paired_count,
    })
}

fn success_delta_confidence_interval(
    pairs: &[(TaskRunAverage, TaskRunAverage)],
    estimate: f64,
) -> ConfidenceInterval {
    let confidence_level = 0.95;
    if pairs.len() < 2 {
        return ConfidenceInterval {
            estimate,
            lower_bound: estimate,
            upper_bound: estimate,
            confidence_level,
        };
    }

    let deltas: Vec<f64> = pairs
        .iter()
        .map(|(baseline, layers)| layers.success_score - baseline.success_score)
        .collect();
    let n = deltas.len() as f64;
    let mean = deltas.iter().sum::<f64>() / n;
    let variance = deltas
        .iter()
        .map(|delta| {
            let centered = delta - mean;
            centered * centered
        })
        .sum::<f64>()
        / (n - 1.0);
    let margin = 1.96 * (variance / n).sqrt();

    ConfidenceInterval {
        estimate,
        lower_bound: estimate - margin,
        upper_bound: estimate + margin,
        confidence_level,
    }
}

fn average_task_runs(runs: &[&WorkflowRun]) -> TaskRunAverage {
    let count = runs.len() as f64;
    TaskRunAverage {
        wall_time_ms: runs.iter().map(|run| run.wall_time_ms as f64).sum::<f64>() / count,
        total_tokens: runs.iter().map(|run| total_tokens(run)).sum::<f64>() / count,
        success_score: runs.iter().map(|run| run.success_score).sum::<f64>() / count,
        human_interventions: runs
            .iter()
            .map(|run| run.human_interventions as f64)
            .sum::<f64>()
            / count,
        tool_calls: runs.iter().map(|run| run.tool_calls as f64).sum::<f64>() / count,
        failed_attempts: runs
            .iter()
            .map(|run| run.failed_attempts as f64)
            .sum::<f64>()
            / count,
        verification_quality: runs
            .iter()
            .map(|run| run.verification_quality as f64)
            .sum::<f64>()
            / count,
        retrieval_quality: runs
            .iter()
            .map(|run| retrieval_quality_score(run))
            .sum::<f64>()
            / count,
        layers_overhead_ms: runs
            .iter()
            .map(|run| run.layers_overhead_ms as f64)
            .sum::<f64>()
            / count,
        layers_overhead_tokens: runs
            .iter()
            .map(|run| run.layers_overhead_tokens as f64)
            .sum::<f64>()
            / count,
        missed_critical_context: runs
            .iter()
            .filter(|run| run.missed_critical_context > 0)
            .count() as f64
            / count,
        hallucinated_or_stale_context: runs
            .iter()
            .filter(|run| run.hallucinated_or_stale_context > 0)
            .count() as f64
            / count,
    }
}

fn format_report(report: &BenchmarkReport, json: bool) -> Result<String> {
    if json {
        return serde_json::to_string_pretty(report)
            .context("failed to serialize benchmark report");
    }

    let mut output = String::new();
    writeln!(output, "Workflow benchmark report")?;
    writeln!(output, "=========================")?;
    writeln!(output, "Runs: {}", report.run_count)?;
    writeln!(output, "Paired tasks: {}", report.paired_task_count)?;
    if let Some(baseline) = &report.baseline {
        write_variant_summary(&mut output, "Baseline", baseline)?;
    }
    if let Some(layers) = &report.layers {
        write_variant_summary(&mut output, "Layers", layers)?;
    }
    if let Some(comparison) = &report.comparison {
        writeln!(output, "\nPaired net benefit")?;
        writeln!(
            output,
            "- Net time saved per task: {:.1} ms",
            comparison.net_time_saved_ms
        )?;
        writeln!(
            output,
            "- Net tokens saved per task: {:.1}",
            comparison.net_tokens_saved
        )?;
        writeln!(output, "- Speedup: {:.3}x", comparison.speedup)?;
        writeln!(
            output,
            "- Token reduction ratio: {:.3}",
            comparison.token_reduction_ratio
        )?;
        writeln!(output, "- Success delta: {:.3}", comparison.success_delta)?;
        writeln!(
            output,
            "- Human intervention delta: {:.3}",
            comparison.human_intervention_delta
        )?;
        writeln!(
            output,
            "- Layers overhead: {:.1} ms / {:.1} tokens",
            comparison.layers_overhead_ms, comparison.layers_overhead_tokens
        )?;
    }
    Ok(output)
}

fn write_variant_summary(
    output: &mut String,
    name: &str,
    aggregate: &VariantAggregate,
) -> Result<()> {
    writeln!(output, "\n{name}")?;
    writeln!(output, "- Runs: {}", aggregate.run_count)?;
    writeln!(output, "- Success rate: {:.3}", aggregate.success_rate)?;
    writeln!(
        output,
        "- Median wall time: {:.1} ms",
        aggregate.median_wall_time_ms
    )?;
    writeln!(
        output,
        "- Average total tokens: {:.1}",
        aggregate.average_total_tokens
    )?;
    writeln!(
        output,
        "- Context relevant/waste ratios: {:.3} / {:.3}",
        aggregate.relevant_context_ratio, aggregate.context_waste_ratio
    )?;
    writeln!(
        output,
        "- Verification/change/planning quality: {:.2} / {:.2} / {:.2}",
        aggregate.average_verification_quality,
        aggregate.average_change_quality,
        aggregate.average_planning_quality
    )?;
    Ok(())
}

fn average_by<F>(runs: &[&WorkflowRun], value: F) -> f64
where
    F: Fn(&WorkflowRun) -> f64,
{
    runs.iter().map(|run| value(run)).sum::<f64>() / runs.len() as f64
}

fn average_pair_delta<F>(pairs: &[(TaskRunAverage, TaskRunAverage)], value: F) -> f64
where
    F: Fn(&TaskRunAverage) -> f64,
{
    pairs
        .iter()
        .map(|(baseline, layers)| value(layers) - value(baseline))
        .sum::<f64>()
        / pairs.len() as f64
}

fn event_rate<F>(runs: &[&WorkflowRun], value: F) -> f64
where
    F: Fn(&WorkflowRun) -> u64,
{
    if runs.is_empty() {
        0.0
    } else {
        runs.iter().filter(|run| value(run) > 0).count() as f64 / runs.len() as f64
    }
}

fn retrieval_quality_score(run: &WorkflowRun) -> f64 {
    let quality = &run.retrieval_quality;
    f64::from(
        quality.relevance
            + quality.completeness
            + quality.specificity
            + quality.freshness
            + quality.grounding
            + quality.concision
            + quality.noise_absence,
    ) / 7.0
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        values[middle - 1].midpoint(values[middle])
    } else {
        values[middle]
    }
}

fn checked_sum<const N: usize>(values: [u64; N], label: &str) -> Result<u64> {
    values.into_iter().try_fold(0_u64, |sum, value| {
        sum.checked_add(value)
            .with_context(|| format!("{label} overflowed u64"))
    })
}

fn total_tokens(run: &WorkflowRun) -> f64 {
    let prompt_and_response_tokens = run.input_tokens as f64 + run.output_tokens as f64;
    if run.variant.is_layers() {
        prompt_and_response_tokens + run.layers_overhead_tokens as f64
    } else {
        prompt_and_response_tokens
    }
}

fn ratio_f64(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn assert_approx_eq(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < f64::EPSILON,
            "expected {actual} to equal {expected}"
        );
    }

    fn valid_run(task_id: &str, variant: &str, wall_time_ms: u64, input_tokens: u64) -> String {
        format!(
            r#"{{"task_id":"{task_id}","variant":"{variant}","task_category":"small_bugfix","success_score":1.0,"wall_time_ms":{wall_time_ms},"orientation_ms":10,"implementation_ms":20,"debugging_ms":5,"verification_ms":5,"input_tokens":{input_tokens},"output_tokens":100,"peak_context_tokens":900,"context_relevant_tokens":600,"context_duplicate_tokens":100,"context_irrelevant_tokens":200,"assistant_turns":4,"tool_calls":10,"failed_commands":1,"patch_attempts":1,"test_runs":2,"human_interventions":0,"failed_attempts":1,"retrieval_quality":{{"relevance":5,"completeness":4,"specificity":5,"freshness":5,"grounding":4,"concision":4,"noise_absence":4}},"verification_quality":5,"change_quality":4,"planning_quality":5,"reproducibility":5,"confidence_calibration":4,"user_usefulness":5,"layers_overhead_ms":0,"layers_overhead_tokens":0,"missed_critical_context":0,"hallucinated_or_stale_context":0,"regressions":0}}"#
        )
    }

    fn negative_control_run(task_id: &str, variant: &str, abstained: bool) -> String {
        let unnecessary_injections = u64::from(variant == "layers" && !abstained);
        valid_run(task_id, variant, 600, 1_000)
            .replace("\"task_category\":\"small_bugfix\"", "\"task_category\":\"negative_control\"")
            .replace("\"peak_context_tokens\":900", "\"peak_context_tokens\":0")
            .replace("\"context_relevant_tokens\":600", "\"context_relevant_tokens\":0")
            .replace("\"context_duplicate_tokens\":100", "\"context_duplicate_tokens\":0")
            .replace("\"context_irrelevant_tokens\":200", "\"context_irrelevant_tokens\":0")
            .replace(
                "\"regressions\":0}",
                &format!(
                    "\"regressions\":0,\"negative_control_abstained\":{abstained},\"unnecessary_context_injections\":{unnecessary_injections},\"context_caused_regressions\":0}}"
                ),
            )
    }

    fn permissive_claim_thresholds() -> ClaimThresholds {
        ClaimThresholds {
            min_paired_tasks: 1,
            min_code_heavy_paired_tasks: 1,
            min_negative_control_paired_tasks: 0,
            min_success_delta: 0.0,
            min_time_saved_ms: 0.0,
            min_token_reduction_ratio: 0.0,
            max_missed_critical_context_rate: 0.0,
            max_hallucinated_or_stale_context_rate: 0.0,
            max_regression_rate: 0.0,
            max_context_caused_regression_rate: 0.0,
            min_negative_control_abstention_rate: 1.0,
            max_unnecessary_context_injection_rate: 0.0,
        }
    }

    fn write_task_spec(path: &Path, task_id: &str, negative_control: bool) {
        let expected_files = if negative_control {
            "[]".to_string()
        } else {
            r#"["src/memory_index/retrieval.rs"]"#.to_string()
        };
        let target_files = expected_files.clone();
        let abstention = if negative_control {
            r#", "abstention_rubric": "Do not inject repository context for this task.""#
        } else {
            ""
        };
        fs::write(
            path,
            format!(
                r#"{{
  "task_id": "{task_id}",
  "title": "Retrieval eval fixture",
  "prompt": "Refactor memory index retrieval fallback tags.",
  "category": "refactor",
  "difficulty": "medium",
  "surface_claim": "layers_targeted_preflight",
  "negative_control": {negative_control},
  "stale_context_trap": false,
  "target_files": {target_files},
  "expected_relevant_files": {expected_files},
  "expected_validation_commands": ["cargo test -q memory_index -- --nocapture"],
  "success_rubric": {{
    "full_success": "Implemented and verified.",
    "partial_success": "Mostly implemented.",
    "failure": "Not implemented.",
    "min_verification_quality": 4,
    "primary_endpoint": "verified_success"
  }}{abstention}
}}"#
            ),
        )
        .expect("write task spec");
    }

    #[test]
    fn builds_retrieval_eval_corpus_from_task_specs_and_repo_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tasks_dir = dir.path().join("tasks");
        fs::create_dir(&tasks_dir).expect("create tasks dir");
        write_task_spec(&tasks_dir.join("code-task.json"), "code-task", false);
        write_task_spec(&tasks_dir.join("negative-task.json"), "negative-task", true);

        let corpus = build_retrieval_eval_corpus(&RetrievalEvalConfig {
            task_path: tasks_dir,
            repo_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        })
        .expect("retrieval eval corpus should build");

        assert_eq!(corpus.pairs.len(), 1);
        assert_eq!(corpus.negative_control_count, 1);
        assert_eq!(corpus.pairs[0].task_id, "code-task");
        assert_eq!(
            corpus.pairs[0].query,
            "Refactor memory index retrieval fallback tags."
        );
        assert_eq!(
            corpus.pairs[0].relevant_ids,
            vec!["file:src/memory_index/retrieval.rs".to_string()]
        );
        assert!(corpus.documents.iter().any(|document| document.id
            == "file:src/memory_index/retrieval.rs"
            && document.path.as_deref() == Some("src/memory_index/retrieval.rs")));
    }

    #[test]
    fn lexical_retrieval_reports_recall_and_mrr() {
        let corpus = RetrievalEvalCorpus {
            pairs: vec![RetrievalEvalPair {
                task_id: "memory-task".to_string(),
                query: "memory retrieval fallback tags".to_string(),
                relevant_ids: vec!["file:src/memory_index/retrieval.rs".to_string()],
                category: "refactor".to_string(),
                stale_context_trap: false,
            }],
            documents: vec![
                RetrievalEvalDocument {
                    id: "file:src/memory_index/retrieval.rs".to_string(),
                    source_kind: "file".to_string(),
                    path: Some("src/memory_index/retrieval.rs".to_string()),
                    text: "retrieval fallback tags for memory index".to_string(),
                },
                RetrievalEvalDocument {
                    id: "file:src/router.rs".to_string(),
                    source_kind: "file".to_string(),
                    path: Some("src/router.rs".to_string()),
                    text: "route classification".to_string(),
                },
            ],
            negative_control_count: 2,
        };

        let report = evaluate_lexical_retrieval(&corpus).expect("lexical eval");

        assert_eq!(report.pair_count, 1);
        assert_eq!(report.document_count, 2);
        assert_eq!(report.negative_control_count, 2);
        assert_approx_eq(report.recall_at_5, 1.0);
        assert_approx_eq(report.recall_at_10, 1.0);
        assert_approx_eq(report.mrr, 1.0);
        assert_eq!(
            report.per_pair[0].top_document_ids[0],
            "file:src/memory_index/retrieval.rs"
        );
        assert_eq!(report.per_pair[0].first_relevant_rank, Some(1));
        assert_approx_eq(report.negative_control_injection_rate, 0.0);
    }

    #[test]
    fn parses_valid_jsonl_runs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("runs.jsonl");
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                valid_run("bugfix-1", "baseline", 1_000, 2_000),
                valid_run("bugfix-1", "layers", 800, 1_500)
            ),
        )
        .expect("write fixture");

        let runs = load_runs(&path).expect("valid fixture should parse");
        assert_eq!(runs.len(), 2);
        assert!(matches!(runs[0].variant, WorkflowVariant::Baseline));
        assert!(matches!(
            runs[1].variant,
            WorkflowVariant::LayersTargetedPreflight
        ));
    }

    #[test]
    fn rejects_invalid_variant_and_quality_scores() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bad_variant = dir.path().join("bad-variant.jsonl");
        fs::write(&bad_variant, valid_run("bugfix-1", "control", 1_000, 2_000))
            .expect("write fixture");
        assert!(load_runs(&bad_variant).is_err());

        let bad_score = dir.path().join("bad-score.jsonl");
        fs::write(
            &bad_score,
            valid_run("bugfix-1", "layers", 1_000, 2_000)
                .replace("\"verification_quality\":5", "\"verification_quality\":6"),
        )
        .expect("write fixture");
        assert!(load_runs(&bad_score).is_err());
    }

    #[test]
    fn computes_variant_aggregates_and_context_ratios() {
        let runs = vec![
            parse_run(&valid_run("a", "baseline", 1_000, 2_000)).expect("baseline a"),
            parse_run(&valid_run("b", "baseline", 3_000, 4_000)).expect("baseline b"),
            parse_run(&valid_run("a", "layers", 500, 1_000)).expect("layers a"),
        ];

        let report = analyze_runs(&runs).expect("analysis");
        let baseline = report.baseline.expect("baseline aggregate");
        let layers = report.layers.expect("layers aggregate");

        assert_eq!(baseline.run_count, 2);
        assert_approx_eq(baseline.median_wall_time_ms, 2_000.0);
        assert_approx_eq(baseline.average_input_tokens, 3_000.0);
        assert_approx_eq(baseline.relevant_context_ratio, 600.0 / 900.0);
        assert_eq!(layers.run_count, 1);
        assert_approx_eq(layers.average_verification_quality, 5.0);
    }

    #[test]
    fn computes_paired_net_benefit_and_ignores_unpaired_runs() {
        let mut layer_run = valid_run("task-1", "layers", 700, 1_500);
        layer_run = layer_run
            .replace("\"layers_overhead_ms\":0", "\"layers_overhead_ms\":50")
            .replace(
                "\"layers_overhead_tokens\":0",
                "\"layers_overhead_tokens\":100",
            );
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&layer_run).expect("layers"),
            parse_run(&valid_run("unpaired", "baseline", 9_000, 9_000)).expect("unpaired"),
        ];

        let report = analyze_runs(&runs).expect("analysis");
        let comparison = report.comparison.expect("comparison");

        assert_eq!(comparison.paired_task_count, 1);
        assert_approx_eq(comparison.net_time_saved_ms, 300.0);
        assert_approx_eq(comparison.net_tokens_saved, 400.0);
        assert_approx_eq(comparison.layers_overhead_ms, 50.0);
        assert_approx_eq(comparison.layers_overhead_tokens, 100.0);
        assert_approx_eq(comparison.success_delta, 0.0);
    }

    #[test]
    fn reports_layers_surfaces_separately() {
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&valid_run(
                "task-1",
                "layers_targeted_preflight",
                700,
                1_500,
            ))
            .expect("targeted"),
            parse_run(&valid_run("task-1", "layers_broad_query", 1_200, 2_400)).expect("broad"),
            parse_run(&valid_run("task-1", "layers_mcp_preflight", 800, 1_700)).expect("mcp"),
        ];

        let report = analyze_runs(&runs).expect("analysis");

        assert_eq!(report.variants.len(), 4);
        assert!(
            report
                .comparisons
                .iter()
                .any(|comparison| comparison.variant == WorkflowVariant::LayersTargetedPreflight)
        );
        assert!(
            report
                .comparisons
                .iter()
                .any(|comparison| comparison.variant == WorkflowVariant::LayersBroadQuery)
        );
        assert!(
            report
                .comparisons
                .iter()
                .any(|comparison| comparison.variant == WorkflowVariant::LayersMcpPreflight)
        );
        assert_eq!(
            report.comparison.expect("default comparison").variant,
            WorkflowVariant::LayersTargetedPreflight
        );
    }

    #[test]
    fn default_claim_thresholds_match_preregistered_gates() {
        let thresholds = ClaimThresholds::default();

        assert_eq!(thresholds.min_paired_tasks, 30);
        assert_eq!(thresholds.min_code_heavy_paired_tasks, 20);
        assert_eq!(thresholds.min_negative_control_paired_tasks, 5);
        assert_approx_eq(thresholds.min_token_reduction_ratio, 0.20);
        assert_approx_eq(thresholds.min_negative_control_abstention_rate, 0.95);
        assert_approx_eq(thresholds.max_unnecessary_context_injection_rate, 0.05);
        assert_approx_eq(thresholds.max_context_caused_regression_rate, 0.0);
    }

    #[test]
    fn claim_is_inconclusive_when_sample_size_is_too_small() {
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&valid_run(
                "task-1",
                "layers_targeted_preflight",
                700,
                1_500,
            ))
            .expect("targeted"),
        ];
        let thresholds = ClaimThresholds {
            min_paired_tasks: 2,
            min_code_heavy_paired_tasks: 2,
            ..permissive_claim_thresholds()
        };

        let report = analyze_runs_with_thresholds(&runs, thresholds).expect("analysis");
        let claim = report.claim.expect("claim report");

        assert_eq!(claim.status, ClaimStatus::Inconclusive);
        assert!(
            claim
                .uncertainty_notes
                .iter()
                .any(|note| note.contains("paired_task_count"))
        );
        assert!(
            claim
                .uncertainty_notes
                .iter()
                .any(|note| note.contains("code_heavy_paired_task_count"))
        );
        assert!(
            report
                .comparison
                .expect("comparison")
                .success_delta_confidence_interval
                .confidence_level
                > 0.0
        );
    }

    #[test]
    fn rejects_inconsistent_timings_context_and_overhead() {
        let invalid_phase = valid_run("task-1", "layers", 30, 2_000);
        let err = parse_run(&invalid_phase).expect_err("phase time exceeds wall time");
        assert!(err.to_string().contains("phase durations"));

        let invalid_context = valid_run("task-1", "layers", 1_000, 2_000).replace(
            "\"context_irrelevant_tokens\":200",
            "\"context_irrelevant_tokens\":300",
        );
        let err = parse_run(&invalid_context).expect_err("classified context exceeds peak");
        assert!(err.to_string().contains("classified context tokens"));

        let invalid_baseline_overhead = valid_run("task-1", "baseline", 1_000, 2_000)
            .replace("\"layers_overhead_ms\":0", "\"layers_overhead_ms\":1");
        let err = parse_run(&invalid_baseline_overhead).expect_err("baseline overhead rejected");
        assert!(err.to_string().contains("baseline runs"));
    }

    #[test]
    fn averages_duplicate_task_replicates_in_paired_comparison() {
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline 1"),
            parse_run(&valid_run("task-1", "baseline", 3_000, 4_000)).expect("baseline 2"),
            parse_run(&valid_run("task-1", "layers", 500, 1_000)).expect("layers 1"),
            parse_run(&valid_run("task-1", "layers", 700, 1_400)).expect("layers 2"),
        ];

        let report = analyze_runs(&runs).expect("analysis");
        let comparison = report.comparison.expect("comparison");

        assert_eq!(comparison.paired_task_count, 1);
        assert_approx_eq(comparison.net_time_saved_ms, 1_400.0);
        assert_approx_eq(comparison.net_tokens_saved, 1_800.0);
        assert_approx_eq(comparison.speedup, 2_000.0 / 600.0);
    }

    #[test]
    fn rejects_unpaired_benchmarks() {
        let runs = vec![
            parse_run(&valid_run("baseline-only", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&valid_run("layers-only", "layers", 700, 1_500)).expect("layers"),
        ];

        let err = analyze_runs(&runs).expect_err("no paired task should fail");
        assert!(err.to_string().contains("at least one task"));
    }

    #[test]
    fn emits_machine_readable_json_report() {
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&valid_run("task-1", "layers", 700, 1_500)).expect("layers"),
        ];
        let output = format_report(&analyze_runs(&runs).expect("analysis"), true).expect("json");
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");
        assert_eq!(value["comparison"]["paired_task_count"], 1);
    }

    #[test]
    fn claim_supported_when_thresholds_are_met() {
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&valid_run("task-1", "layers", 700, 1_500)).expect("layers"),
            parse_run(&negative_control_run("neg-1", "baseline", false)).expect("baseline neg"),
            parse_run(&negative_control_run("neg-1", "layers", true)).expect("layers neg"),
        ];
        let report =
            analyze_runs_with_thresholds(&runs, permissive_claim_thresholds()).expect("analysis");
        let claim = report.claim.expect("claim report");
        assert_eq!(claim.status, ClaimStatus::Supported);
    }

    #[test]
    fn claim_not_supported_when_success_regresses() {
        let mut layer_run = valid_run("task-1", "layers", 700, 1_500);
        layer_run = layer_run.replace("\"success_score\":1.0", "\"success_score\":0.5");
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&layer_run).expect("layers"),
        ];
        let report =
            analyze_runs_with_thresholds(&runs, permissive_claim_thresholds()).expect("analysis");
        let claim = report.claim.expect("claim report");
        assert_eq!(claim.status, ClaimStatus::NotSupported);
        assert!(
            claim
                .blocking_metrics
                .iter()
                .any(|metric| metric == "success_delta")
        );
    }

    #[test]
    fn negative_control_without_abstention_blocks_claim() {
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&valid_run("task-1", "layers", 700, 1_500)).expect("layers"),
            parse_run(&negative_control_run("neg-1", "baseline", false)).expect("baseline neg"),
            parse_run(&negative_control_run("neg-1", "layers", false)).expect("layers neg"),
        ];
        let report =
            analyze_runs_with_thresholds(&runs, permissive_claim_thresholds()).expect("analysis");
        let claim = report.claim.expect("claim report");
        assert_eq!(claim.status, ClaimStatus::NotSupported);
        assert!(
            claim
                .blocking_metrics
                .iter()
                .any(|metric| metric == "negative_control_abstention_rate")
        );
    }

    #[test]
    fn claim_rates_are_finite_without_negative_control_runs() {
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&valid_run("task-1", "layers", 700, 1_500)).expect("layers"),
        ];
        let report =
            analyze_runs_with_thresholds(&runs, permissive_claim_thresholds()).expect("analysis");
        let claim = report.claim.expect("claim report");

        assert!(claim.negative_control_abstention_rate.is_finite());
        assert!(claim.unnecessary_context_injection_rate.is_finite());
        assert!(claim.context_caused_regression_rate.is_finite());
        assert_approx_eq(claim.unnecessary_context_injection_rate, 0.0);
        assert_approx_eq(claim.context_caused_regression_rate, 0.0);
    }

    #[test]
    fn emits_human_report_with_core_benchmark_fields() {
        let runs = vec![
            parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).expect("baseline"),
            parse_run(&valid_run("task-1", "layers", 700, 1_500)).expect("layers"),
        ];
        let output = format_report(&analyze_runs(&runs).expect("analysis"), false).expect("human");
        assert!(output.contains("Workflow benchmark report"));
        assert!(output.contains("Paired net benefit"));
        assert!(output.contains("Context relevant/waste ratios"));
    }

    #[test]
    fn parses_and_validates_task_spec_fixture() {
        let spec = load_task_spec(Path::new(
            "benchmarks/workflows/fixtures/valid-task-spec.json",
        ))
        .expect("valid task spec fixture");

        assert_eq!(spec.task_id, "fixture-valid-code-task");
        assert_eq!(spec.surface_claim, SurfaceClaim::LayersTargetedPreflight);
        assert_eq!(spec.success_rubric.primary_endpoint, "verified_success");
    }

    #[test]
    fn plans_isolated_runner_artifacts_for_paired_agent_runs() {
        let output = tempfile::tempdir().expect("temp output dir");
        let config = RunnerPlanConfig {
            task_path: PathBuf::from("benchmarks/workflows/fixtures/valid-task-spec.json"),
            output_dir: output.path().join("phase11-run"),
            repo_root: PathBuf::from("/repo/layers"),
            agent_command: "codex exec".to_owned(),
            model: Some("test-model".to_owned()),
            seed: 11,
        };

        let plan = plan_runner_artifacts(&config).expect("runner plan artifacts");

        assert_eq!(plan.variants, vec!["baseline", "layers_targeted_preflight"]);
        assert_eq!(plan.runs.len(), 2);
        assert!(plan.runs.iter().any(|run| run.variant == "baseline"));
        assert!(
            plan.runs
                .iter()
                .any(|run| run.variant == "layers_targeted_preflight")
        );
        for run in &plan.runs {
            assert!(run.worktree_path.starts_with(&plan.worktree_root));
            assert!(run.transcript_path.starts_with(&config.output_dir));
            assert!(run.prompt_path.starts_with(&config.output_dir));
            assert!(run.validation_log_path.starts_with(&config.output_dir));
            assert_eq!(run.agent_command, "codex exec");
            assert_eq!(run.model.as_deref(), Some("test-model"));
        }
        let targeted = plan
            .runs
            .iter()
            .find(|run| run.variant == "layers_targeted_preflight")
            .expect("targeted run");
        assert!(targeted.requires_layers_preflight);
        assert!(targeted.packet_artifact_path.is_some());
        let baseline = plan
            .runs
            .iter()
            .find(|run| run.variant == "baseline")
            .expect("baseline run");
        assert!(!baseline.requires_layers_preflight);
        assert!(baseline.packet_artifact_path.is_none());

        let plan_json = fs::read_to_string(config.output_dir.join("runner-plan.json"))
            .expect("runner plan JSON written");
        let value: serde_json::Value = serde_json::from_str(&plan_json).expect("plan JSON parses");
        assert_eq!(value["runs"].as_array().expect("runs array").len(), 2);
        let order_jsonl = fs::read_to_string(config.output_dir.join("execution-order.jsonl"))
            .expect("execution order JSONL written");
        assert_eq!(order_jsonl.lines().count(), 2);
    }

    #[test]
    fn executes_runner_plan_with_isolated_worktrees_and_run_records() {
        let output = tempfile::tempdir().expect("temp output dir");
        let repo = tempfile::tempdir().expect("temp repo dir");
        let task_path = output.path().join("phase12-smoke-task.json");
        fs::write(
            &task_path,
            r#"{
  "task_id": "phase12-smoke-runner-execution-test",
  "title": "Phase 12 smoke runner execution test",
  "prompt": "Write agent-output.txt in the working directory.",
  "category": "bugfix",
  "difficulty": "small",
  "surface_claim": "layers_targeted_preflight",
  "negative_control": false,
  "stale_context_trap": false,
  "repo_commit": "HEAD",
  "time_budget_minutes": 1,
  "target_files": ["agent-output.txt"],
  "target_symbols": ["agent-output"],
  "expected_relevant_files": ["agent-output.txt"],
  "expected_validation_commands": ["test -f agent-output.txt"],
  "success_rubric": {
    "full_success": "agent-output.txt exists.",
    "partial_success": "The agent ran but did not write the expected file.",
    "failure": "The agent did not run or validation failed.",
    "min_verification_quality": 4,
    "primary_endpoint": "verified_success"
  }
}
"#,
        )
        .expect("task written");
        let config = RunnerPlanConfig {
            task_path,
            output_dir: output.path().join("phase12-run"),
            repo_root: repo.path().to_path_buf(),
            agent_command: "python3 -c \"from pathlib import Path; Path('agent-output.txt').write_text('done')\"".to_owned(),
            model: Some("smoke-model".to_owned()),
            seed: 12,
        };
        let plan = plan_runner_artifacts(&config).expect("runner plan artifacts");
        let execution = execute_runner_plan(&RunnerExecutionConfig {
            plan_path: plan.plan_path.clone(),
            preflight_command: "python3 -c \"import json; print(json.dumps({'packet':'ok'}))\""
                .to_owned(),
            keep_worktrees: true,
        })
        .expect("runner execution");

        assert_eq!(execution.total_runs, 2);
        assert_eq!(execution.completed_runs, 2);
        assert_eq!(execution.failed_runs, 0);
        assert!(execution.run_records_path.starts_with(&config.output_dir));
        let run_records =
            fs::read_to_string(&execution.run_records_path).expect("run records written");
        assert_eq!(run_records.lines().count(), 2);
        let parsed_runs = run_records
            .lines()
            .map(parse_run)
            .collect::<Result<Vec<_>>>()
            .expect("run records parse");
        assert!(
            parsed_runs
                .iter()
                .any(|run| run.variant == WorkflowVariant::Baseline)
        );
        assert!(
            parsed_runs
                .iter()
                .any(|run| run.variant == WorkflowVariant::LayersTargetedPreflight)
        );
        for run in &plan.runs {
            assert!(
                run.worktree_path.exists(),
                "worktree exists for {}",
                run.run_id
            );
            assert!(run.worktree_path.join("agent-output.txt").exists());
            assert!(
                fs::read_to_string(&run.transcript_path)
                    .expect("transcript updated")
                    .contains("Agent exit status: 0")
            );
            assert!(
                fs::read_to_string(&run.validation_log_path)
                    .expect("validation log written")
                    .contains("Validation command")
            );
        }
        let targeted = plan
            .runs
            .iter()
            .find(|run| run.requires_layers_preflight)
            .expect("targeted run");
        assert!(
            targeted
                .packet_artifact_path
                .as_ref()
                .expect("packet path")
                .exists()
        );
    }

    #[test]
    fn runner_validation_failure_marks_run_incomplete() {
        let output = tempfile::tempdir().expect("temp output dir");
        let repo = tempfile::tempdir().expect("temp repo dir");
        let config = RunnerPlanConfig {
            task_path: PathBuf::from("benchmarks/workflows/fixtures/valid-task-spec.json"),
            output_dir: output.path().join("phase12-run"),
            repo_root: repo.path().to_path_buf(),
            agent_command: "python3 -c \"from pathlib import Path; Path('agent-output.txt').write_text('done')\"".to_owned(),
            model: None,
            seed: 14,
        };
        let mut plan = plan_runner_artifacts(&config).expect("runner plan artifacts");
        for run in &mut plan.runs {
            run.expected_validation_commands = vec!["false".to_owned()];
        }
        fs::write(
            &plan.plan_path,
            serde_json::to_string_pretty(&plan).expect("plan serializes"),
        )
        .expect("plan rewritten");

        let execution = execute_runner_plan(&RunnerExecutionConfig {
            plan_path: plan.plan_path.clone(),
            preflight_command: "python3 -c \"print('{}')\"".to_owned(),
            keep_worktrees: true,
        })
        .expect("runner execution");

        assert_eq!(execution.completed_runs, 0);
        assert_eq!(execution.failed_runs, 2);
        let records = fs::read_to_string(&execution.run_records_path).expect("run records written");
        for record in records.lines().map(parse_run) {
            let record = record.expect("run record parses");
            assert!(record.success_score.abs() < f64::EPSILON);
            assert!(record.failed_commands > 0);
            assert!(record.failed_attempts > 0);
        }
    }

    #[test]
    fn runner_cleanup_removes_worktrees_when_not_kept() {
        let output = tempfile::tempdir().expect("temp output dir");
        let repo = tempfile::tempdir().expect("temp repo dir");
        let config = RunnerPlanConfig {
            task_path: PathBuf::from("benchmarks/workflows/fixtures/valid-task-spec.json"),
            output_dir: output.path().join("phase12-run"),
            repo_root: repo.path().to_path_buf(),
            agent_command: "python3 -c \"from pathlib import Path; Path('agent-output.txt').write_text('done')\"".to_owned(),
            model: None,
            seed: 13,
        };
        let plan = plan_runner_artifacts(&config).expect("runner plan artifacts");
        execute_runner_plan(&RunnerExecutionConfig {
            plan_path: plan.plan_path.clone(),
            preflight_command: "python3 -c \"print('{}')\"".to_owned(),
            keep_worktrees: false,
        })
        .expect("runner execution");

        for run in &plan.runs {
            assert!(
                !run.worktree_path.exists(),
                "worktree cleaned for {}",
                run.run_id
            );
        }
    }

    #[test]
    fn runner_prompts_keep_baseline_free_of_layers_context() {
        let output = tempfile::tempdir().expect("temp output dir");
        let config = RunnerPlanConfig {
            task_path: PathBuf::from("benchmarks/workflows/fixtures/valid-task-spec.json"),
            output_dir: output.path().join("phase11-run"),
            repo_root: PathBuf::from("/repo/layers"),
            agent_command: "claude -p".to_owned(),
            model: None,
            seed: 7,
        };

        let plan = plan_runner_artifacts(&config).expect("runner plan artifacts");
        let baseline = plan
            .runs
            .iter()
            .find(|run| run.variant == "baseline")
            .expect("baseline run");
        let baseline_prompt =
            fs::read_to_string(&baseline.prompt_path).expect("baseline prompt should be written");
        assert!(baseline_prompt.contains("Do not run Layers commands"));
        assert!(!baseline_prompt.contains("layers preflight --no-audit --json --strict"));

        let targeted = plan
            .runs
            .iter()
            .find(|run| run.variant == "layers_targeted_preflight")
            .expect("targeted run");
        let targeted_prompt =
            fs::read_to_string(&targeted.prompt_path).expect("targeted prompt should be written");
        assert!(
            targeted_prompt.contains("benchmark harness handles the Layers targeted-preflight")
        );
        assert!(targeted_prompt.contains("do not run additional `layers preflight` commands"));
        assert!(targeted_prompt.contains("harness-generated targeted-preflight packet artifact"));
    }

    #[test]
    fn run_protocol_artifacts_define_reproducible_benchmark_runs() {
        let protocol = fs::read_to_string("benchmarks/workflows/RUN_PROTOCOL.md")
            .expect("run protocol should exist");
        for required in [
            "Checkout/reset procedure",
            "Agent/model used",
            "Tool permissions",
            "Time budget",
            "Randomized order",
            "Baseline prompt format",
            "Targeted preflight prompt format",
            "Saving packet artifacts",
            "Scoring success",
            "Tokens, tool calls, and time",
            "Missed critical context",
            "Stale context",
            "Unnecessary context injection",
        ] {
            assert!(
                protocol.contains(required),
                "run protocol should cover {required}"
            );
        }

        let run_template =
            fs::read_to_string("benchmarks/workflows/templates/workflow-run-record.json")
                .expect("workflow run record template should exist");
        let run = parse_run(&run_template).expect("workflow run record template should parse");
        assert_eq!(run.variant, WorkflowVariant::LayersTargetedPreflight);
        assert_eq!(run.task_id, "<task_id>");

        let transcript_template =
            fs::read_to_string("benchmarks/workflows/templates/transcript-template.md")
                .expect("transcript template should exist");
        for heading in [
            "# Workflow Benchmark Transcript",
            "## Setup",
            "## Prompt",
            "## Packet Artifacts",
            "## Timeline",
            "## Validation",
            "## Scoring Notes",
            "## Context Quality Classification",
        ] {
            assert!(
                transcript_template.contains(heading),
                "transcript template should contain {heading}"
            );
        }
    }

    #[test]
    fn rejects_task_spec_missing_success_rubric() {
        let err = load_task_spec(Path::new(
            "benchmarks/workflows/fixtures/invalid-task-spec-missing-rubric.json",
        ))
        .expect_err("missing rubric should be rejected");

        assert!(format!("{err:?}").contains("success_rubric"));
    }

    fn valid_task_spec_for_validation_tests() -> TaskSpec {
        TaskSpec {
            task_id: "valid-code-task".to_owned(),
            title: "Valid code task".to_owned(),
            prompt: "Prompt".to_owned(),
            category: "bugfix".to_owned(),
            difficulty: Some("small".to_owned()),
            surface_claim: SurfaceClaim::LayersTargetedPreflight,
            negative_control: false,
            stale_context_trap: false,
            repo_commit: Some("HEAD".to_owned()),
            time_budget_minutes: Some(30),
            target_files: vec!["src/lib.rs".to_owned()],
            target_symbols: vec!["validate_task_spec".to_owned()],
            expected_relevant_files: vec!["src/lib.rs".to_owned()],
            expected_validation_commands: vec!["cargo check".to_owned()],
            success_rubric: SuccessRubric {
                full_success: "Full".to_owned(),
                partial_success: "Partial".to_owned(),
                failure: "Failure".to_owned(),
                min_verification_quality: 4,
                primary_endpoint: "verified_success".to_owned(),
            },
            abstention_rubric: None,
        }
    }

    fn valid_negative_control_task_spec_for_validation_tests() -> TaskSpec {
        TaskSpec {
            task_id: "valid-negative-control-task".to_owned(),
            title: "Valid negative control task".to_owned(),
            prompt: "Prompt".to_owned(),
            category: "negative_control".to_owned(),
            difficulty: Some("trivial".to_owned()),
            surface_claim: SurfaceClaim::LayersTargetedPreflight,
            negative_control: true,
            stale_context_trap: false,
            repo_commit: Some("HEAD".to_owned()),
            time_budget_minutes: Some(5),
            target_files: Vec::new(),
            target_symbols: Vec::new(),
            expected_relevant_files: Vec::new(),
            expected_validation_commands: vec!["cargo check".to_owned()],
            success_rubric: SuccessRubric {
                full_success: "Full".to_owned(),
                partial_success: "Partial".to_owned(),
                failure: "Failure".to_owned(),
                min_verification_quality: 4,
                primary_endpoint: "verified_success".to_owned(),
            },
            abstention_rubric: Some("Layers should abstain from injecting context.".to_owned()),
        }
    }

    #[test]
    fn rejects_task_spec_schema_constraint_mismatches() {
        let mut spec = valid_task_spec_for_validation_tests();
        spec.task_id = "BadTask".to_owned();
        spec.difficulty = Some("tiny".to_owned());
        spec.success_rubric.primary_endpoint = "token_savings".to_owned();

        let err = validate_task_spec(&spec).expect_err("schema constraints should be enforced");
        assert!(format!("{err:?}").contains("task_id"));

        let unknown = serde_json::json!({
            "task_id": "unknown-field-task",
            "title": "Unknown field task",
            "prompt": "Prompt",
            "category": "bugfix",
            "target_files": ["src/lib.rs"],
            "expected_relevant_files": ["src/lib.rs"],
            "expected_validation_commands": ["cargo check"],
            "success_rubric": {
                "full_success": "Full",
                "partial_success": "Partial",
                "failure": "Failure",
                "min_verification_quality": 4,
                "primary_endpoint": "verified_success"
            },
            "unexpected": true
        });
        assert!(serde_json::from_value::<TaskSpec>(unknown).is_err());

        let negative_control_without_abstention = TaskSpec {
            task_id: "negative-control-with-context".to_owned(),
            title: "Negative control with context".to_owned(),
            prompt: "Prompt".to_owned(),
            category: "negative_control".to_owned(),
            difficulty: Some("trivial".to_owned()),
            surface_claim: SurfaceClaim::LayersTargetedPreflight,
            negative_control: true,
            stale_context_trap: false,
            repo_commit: None,
            time_budget_minutes: None,
            target_files: vec!["src/lib.rs".to_owned()],
            target_symbols: Vec::new(),
            expected_relevant_files: Vec::new(),
            expected_validation_commands: vec!["cargo check".to_owned()],
            success_rubric: SuccessRubric {
                full_success: "Full".to_owned(),
                partial_success: "Partial".to_owned(),
                failure: "Failure".to_owned(),
                min_verification_quality: 4,
                primary_endpoint: "verified_success".to_owned(),
            },
            abstention_rubric: None,
        };
        let err = validate_task_spec(&negative_control_without_abstention)
            .expect_err("negative controls with context must require abstention rubric");
        assert!(format!("{err:?}").contains("abstention_rubric"));
    }

    #[test]
    fn rejects_task_spec_schema_scalar_parity_mismatches() {
        let mut spec = valid_task_spec_for_validation_tests();
        spec.repo_commit = Some(" ".to_owned());
        let err = validate_task_spec(&spec).expect_err("blank repo_commit should fail");
        assert!(format!("{err:?}").contains("repo_commit must not be empty"));

        let mut spec = valid_task_spec_for_validation_tests();
        spec.time_budget_minutes = Some(0);
        let err = validate_task_spec(&spec).expect_err("zero time_budget_minutes should fail");
        assert!(format!("{err:?}").contains("time_budget_minutes must be at least 1"));

        let mut spec = valid_task_spec_for_validation_tests();
        spec.target_symbols = vec!["validate_task_spec".to_owned(), " ".to_owned()];
        let err = validate_task_spec(&spec).expect_err("blank target_symbols entry should fail");
        assert!(format!("{err:?}").contains("target_symbols[1] must not be empty"));
    }

    #[test]
    fn rejects_blank_context_file_entries_even_for_negative_controls() {
        let mut spec = valid_negative_control_task_spec_for_validation_tests();
        spec.target_files = vec![" ".to_owned()];
        let err = validate_task_spec(&spec).expect_err("blank target_files entry should fail");
        assert!(format!("{err:?}").contains("target_files[0] must not be empty"));

        let mut spec = valid_negative_control_task_spec_for_validation_tests();
        spec.expected_relevant_files = vec![" ".to_owned()];
        let err =
            validate_task_spec(&spec).expect_err("blank expected_relevant_files entry should fail");
        assert!(format!("{err:?}").contains("expected_relevant_files[0] must not be empty"));
    }

    #[test]
    fn allows_negative_control_with_empty_context_file_arrays() {
        let spec = valid_negative_control_task_spec_for_validation_tests();
        validate_task_spec(&spec).expect("negative controls may omit context file targets");
    }

    #[test]
    fn validates_task_directory_and_reports_invalid_specs() {
        let report = validate_task_specs(Path::new("benchmarks/workflows/fixtures"))
            .expect("fixture directory validation report");

        assert_eq!(report.checked_count, 2);
        assert_eq!(report.valid_count, 1);
        assert_eq!(report.invalid_count, 1);
        assert!(report.results.iter().any(|result| {
            !result.valid
                && result
                    .errors
                    .iter()
                    .any(|error| error.contains("success_rubric"))
        }));
    }
}
