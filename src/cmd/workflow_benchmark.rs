//! Workflow benchmark telemetry analysis for comparing Layers vs baseline runs.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
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
