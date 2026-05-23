//! Automatic local research for task-specific context packets.

use anyhow::{Context, Result, anyhow};
use layers_compiler::{CompileMode, CompileRequest, ContextCompiler};
use layers_core::{
    ContextBudget, ContextItem, ContextPacket, ContextWarning, InjectionRecommendation,
    PacketQualityReport, RetrievalReport, SpecificInstruction, SuccessRubric, TaskCategory,
    TaskSpec,
};
use serde_json::json;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::cmd::autoresearch::{AutoresearchPacketBridgeOptions, add_autoresearch_to_packet};
use crate::cmd::impact::{ImpactSource, ImpactStatus, build_impact_report};
use crate::cmd::packet::format_objective_brief;
use crate::config::{canonical_curated_memory_path, workspace_root};
use crate::context_packet_compiler::{
    add_workspace_section, cited_item, code_impact_section, collect_workspace_state,
    context_section, git_output, repo_source, workspace_id,
};

const DEFAULT_BUDGET_WORDS: usize = 700;
const MAX_MEMORY_ITEMS: usize = 2;
const MAX_CODE_ITEMS: usize = 4;
const COMPACT_MEMORY_ITEM_CHARS: usize = 140;
const COMPACT_CODE_EXCERPT_LINES: usize = 12;
const COMPACT_CODE_EXCERPT_CHARS: usize = 500;

/// Arguments for the `layers preflight` command.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct PreflightArgs {
    /// Task or question to research before editing.
    pub task: String,
    /// Optional user-provided files, directories, symbols, or tests.
    pub targets: Vec<String>,
    /// Output structured JSON.
    pub json: bool,
    /// Output an agent-ready prompt.
    pub agent_prompt: bool,
    /// Skip audit side effects. Reserved for parity with query workflows.
    pub no_audit: bool,
    /// Fail if minimum context coverage is not met.
    pub strict: bool,
}

/// Run local preflight and print the requested packet format.
pub fn handle_preflight(args: &PreflightArgs) -> Result<()> {
    let packet = build_preflight_packet(args)?;
    if args.strict && !minimum_bar_passes(&packet) {
        return Err(anyhow!(
            "preflight did not meet the minimum context bar; rerun without --strict to inspect warnings"
        ));
    }
    if args.strict && !injection_policy_passes(&packet) {
        return Err(anyhow!(
            "preflight packet quality gate recommended abstaining or requesting explicit targets; rerun without --strict to inspect packet_quality"
        ));
    }
    if args.agent_prompt {
        println!("{}", render_preflight_agent_prompt(&packet));
    } else if args.json {
        println!("{}", serde_json::to_string_pretty(&packet)?);
    } else {
        println!("{}", packet.to_markdown());
    }
    Ok(())
}

fn render_preflight_agent_prompt(packet: &ContextPacket) -> String {
    format_objective_brief(packet)
}

fn build_preflight_packet(args: &PreflightArgs) -> Result<ContextPacket> {
    let workspace = workspace_root();
    let workspace_id = workspace_id(&workspace);
    let packet_id = format!("preflight-{}", uuid::Uuid::new_v4());
    let generated_at = chrono::Utc::now();
    let mut packet = ContextPacket::new(
        packet_id.clone(),
        workspace_id.clone(),
        args.task.clone(),
        generated_at,
    );
    packet.task.clone_from(&args.task);
    packet.budget = ContextBudget {
        max_units: DEFAULT_BUDGET_WORDS,
        used_units: 0,
        unit: "words".to_string(),
        truncated: false,
    };

    let explicit_targets = args.targets.clone();
    let inferred_targets = infer_targets(&args.task, &explicit_targets, &workspace);
    let code_heavy = is_code_heavy_task(&args.task, &inferred_targets);
    let workspace_state = collect_workspace_state(&workspace);
    packet.git_ref.clone_from(&workspace_state.head);

    add_workspace_section(&mut packet, &workspace_state);
    add_context_policy_section(&mut packet, &workspace);
    add_memory_section(&mut packet, &args.task);
    let autoresearch_findings =
        add_autoresearch_section(&mut packet, &args.task, &inferred_targets);
    add_code_section(&mut packet, &workspace, &inferred_targets, code_heavy);
    add_impact_section(&mut packet, &workspace, &inferred_targets);
    add_validation_section(&mut packet, code_heavy, &inferred_targets);
    add_preflight_summary_section(&mut packet, args, code_heavy, &inferred_targets);
    add_specific_instructions(&mut packet, &args.task);

    packet.budget.used_units = estimate_packet_words(&packet);
    if packet.budget.used_units > packet.budget.max_units {
        packet.budget.truncated = true;
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "budget_may_be_exceeded".to_string(),
            message: format!(
                "Preflight packet is estimated at {} words, above the {} word budget.",
                packet.budget.used_units, packet.budget.max_units
            ),
        });
    }

    if !args.no_audit {
        packet.warnings.push(ContextWarning {
            severity: "info".to_string(),
            code: "audit_not_yet_persisted".to_string(),
            message: "Preflight audit persistence is not enabled in this beta implementation."
                .to_string(),
        });
    }

    let draft_scores = packet.scores;
    let draft_budget_truncated = packet.budget.truncated;
    let draft_specific_instructions = packet.specific_instructions.clone();
    let mut packet = ContextCompiler::new().compile(
        CompileRequest::new(
            packet_id,
            workspace_id,
            args.task.clone(),
            generated_at,
            CompileMode::Preflight,
        )
        .with_git_ref(workspace_state.head.clone())
        .with_sections(packet.sections)
        .with_warnings(packet.warnings)
        .with_specific_instructions(draft_specific_instructions)
        .derive_evidence(false),
    );
    packet.task.clone_from(&args.task);
    packet.scores = draft_scores;
    packet.budget = ContextBudget {
        max_units: DEFAULT_BUDGET_WORDS,
        used_units: estimate_packet_words(&packet),
        unit: "words".to_string(),
        truncated: draft_budget_truncated,
    };
    if packet.budget.used_units > packet.budget.max_units {
        packet.budget.truncated = true;
    }
    packet.retrieval = RetrievalReport {
        memory_source: "curated-memory-jsonl".to_string(),
        memory_latency_ms: 0,
        graph_latency_ms: 0,
        // Preflight's local workspace/memory/file collectors are the primary
        // implementation path, not a degraded fallback. Leaving this unset keeps
        // confidence and injection-policy reasons aligned for well-grounded
        // targeted packets.
        fallback_reason: None,
    };
    packet.retrieval_meta = packet.retrieval.clone();
    packet.scores = json!({
        "code_heavy": code_heavy,
        "targets": inferred_targets.len(),
        "autoresearch_findings": autoresearch_findings,
        "workspace_dirty": workspace_state.dirty,
    });
    packet.why_retrieved = "Preflight compiles local pre-edit context from workspace state, persisted autoresearch findings, memory, code targets, and validation policy.".to_string();
    packet.why_not_retrieved = String::new();
    packet.confidence = ResearchQuality::new(
        code_heavy,
        has_section(&packet, "memory"),
        has_section(&packet, "code"),
        has_warning(&packet, "low_memory_relevance"),
    )
    .confidence;
    let task_spec = task_spec_for_preflight(args, code_heavy, &inferred_targets);
    add_packet_quality_report(&mut packet, &task_spec);
    Ok(packet)
}

fn has_section(packet: &ContextPacket, section_id: &str) -> bool {
    packet
        .sections
        .iter()
        .any(|section| section.id == section_id)
}

fn has_warning(packet: &ContextPacket, code: &str) -> bool {
    packet.warnings.iter().any(|warning| warning.code == code)
}

fn minimum_bar_passes(packet: &ContextPacket) -> bool {
    has_section(packet, "workspace")
        && has_section(packet, "memory")
        && has_section(packet, "validation")
        && (!is_packet_code_heavy(packet) || has_section(packet, "code"))
        && packet.confidence != "low"
}

fn injection_policy_passes(packet: &ContextPacket) -> bool {
    let recommendation = packet
        .scores
        .get("injection_recommendation")
        .and_then(serde_json::Value::as_str);
    !matches!(recommendation, Some("abstain" | "needs_target"))
}

fn task_spec_for_preflight(args: &PreflightArgs, code_heavy: bool, targets: &[String]) -> TaskSpec {
    let category = if code_heavy {
        TaskCategory::Debugging
    } else {
        TaskCategory::Orientation
    };
    let target_paths = targets.iter().map(PathBuf::from).collect::<Vec<_>>();
    TaskSpec {
        task_id: "preflight-task".to_string(),
        title: "Preflight task".to_string(),
        prompt: args.task.clone(),
        category,
        repo_root: Some(workspace_root()),
        target_files: target_paths.clone(),
        target_symbols: Vec::new(),
        expected_relevant_files: target_paths,
        expected_validation_commands: infer_validation_commands(code_heavy, targets)
            .into_iter()
            .map(|command| command.command)
            .collect(),
        negative_control: false,
        success_rubric: SuccessRubric::default(),
    }
}

fn add_packet_quality_report(packet: &mut ContextPacket, task_spec: &TaskSpec) {
    let report = PacketQualityReport::grade(packet, task_spec);
    packet.scores = json!({
        "preflight": packet.scores.clone(),
        "packet_quality": &report,
        "injection_recommendation": report.recommendation,
    });
    packet.warnings.push(ContextWarning {
        severity: match report.recommendation {
            InjectionRecommendation::InjectFull | InjectionRecommendation::InjectCompact => "info",
            InjectionRecommendation::Abstain | InjectionRecommendation::NeedsTarget => "warning",
        }
        .to_string(),
        code: "injection_policy".to_string(),
        message: format!(
            "Packet quality gate recommends {:?}: {}",
            report.recommendation,
            report.reasons.join("; ")
        ),
    });
}

fn is_packet_code_heavy(packet: &ContextPacket) -> bool {
    packet
        .scores
        .get("code_heavy")
        .or_else(|| {
            packet
                .scores
                .get("preflight")
                .and_then(|scores| scores.get("code_heavy"))
        })
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// Extract imperative instructions from curated memory that match the task.
///
/// Reads `memoryport/curated-memory.jsonl`, filters for constraint/pitfall/learning
/// records whose text shares keywords with the task, and whose text uses imperative
/// language. At most 3 instructions are added to the packet.
pub fn add_specific_instructions(packet: &mut ContextPacket, task: &str) {
    let memory_path = canonical_curated_memory_path();
    let Ok(contents) = std::fs::read_to_string(&memory_path) else {
        return;
    };

    let task_keywords = keywords(task);
    let imperative_prefixes = [
        "do ", "do not", "don't", "always", "never", "must", "avoid", "ensure", "require",
    ];

    let instructions: Vec<SpecificInstruction> = contents
        .lines()
        .filter_map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).ok()?;

            // Extract kind from "entity" or "kind" field.
            let kind = record
                .get("entity")
                .or_else(|| record.get("kind"))
                .and_then(serde_json::Value::as_str)?
                .to_string();

            // Filter for constraint, pitfall, or learning kinds.
            if !matches!(kind.as_str(), "constraint" | "pitfall" | "learning") {
                return None;
            }

            // Extract text from "text" or "payload.summary".
            let text = record
                .get("text")
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    record
                        .get("payload")
                        .and_then(|p| p.get("summary"))
                        .and_then(serde_json::Value::as_str)
                })?
                .to_string();

            // Check if the text shares keywords with the task.
            if keyword_score(&text, &task_keywords) == 0 {
                return None;
            }

            // Extract confidence, defaulting to "high" for curated records.
            let confidence = record
                .get("confidence")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("high")
                .to_string();

            // Only accept "medium" or "high" confidence.
            if !matches!(confidence.as_str(), "medium" | "high") {
                return None;
            }

            // Check for imperative language (text starts with an imperative prefix).
            let text_lower = text.to_lowercase();
            let is_imperative = imperative_prefixes
                .iter()
                .any(|prefix| text_lower.starts_with(prefix));
            if !is_imperative {
                return None;
            }

            // Extract record ID.
            let source_id = record
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_string();

            Some(SpecificInstruction {
                text,
                source_id,
                kind,
                confidence,
            })
        })
        .take(3)
        .collect();

    packet.specific_instructions = instructions;
}

fn add_preflight_summary_section(
    packet: &mut ContextPacket,
    args: &PreflightArgs,
    code_heavy: bool,
    targets: &[String],
) {
    let body = format!(
        "Preflight classified this task as {}. Explicit targets: {}. Effective targets: {}.",
        if code_heavy {
            "code-heavy"
        } else {
            "memory-oriented"
        },
        if args.targets.is_empty() {
            "none".to_string()
        } else {
            args.targets.join(", ")
        },
        if targets.is_empty() {
            "none".to_string()
        } else {
            targets.join(", ")
        }
    );
    packet.sections.insert(
        0,
        context_section(
            "preflight_summary",
            "Preflight Summary",
            "Preflight planning result.",
            vec![context_item(
                "research-summary-1",
                "Preflight classification",
                &body,
                "preflight",
                "local-planner",
                None,
                "summarizes the local preflight plan before source collection",
                vec!["planning".to_string()],
            )],
        ),
    );
}

fn add_context_policy_section(packet: &mut ContextPacket, workspace: &Path) {
    let policy_path = workspace.join("LAYERS.md");
    let Ok(body) = read_file_excerpt(&policy_path) else {
        return;
    };
    packet.sections.push(context_section(
        "context_policy",
        "Repo-Owned Context Policy",
        "Checked-in instructions that constrain packet compilation and downstream agent work.",
        vec![context_item(
            "context-policy-1",
            "LAYERS.md",
            &body,
            "context_policy",
            "LAYERS.md",
            Some("LAYERS.md".to_string()),
            "repo-owned context policy should constrain this preflight packet before agent work begins",
            vec!["policy".to_string(), "repo-owned".to_string()],
        )],
    ));
}

fn add_memory_section(packet: &mut ContextPacket, task: &str) {
    let memory_path = canonical_curated_memory_path();
    let Ok(contents) = std::fs::read_to_string(&memory_path) else {
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "memory_unavailable".to_string(),
            message: format!(
                "Could not read curated memory at {}.",
                memory_path.display()
            ),
        });
        return;
    };
    let keywords = keywords(task);
    let mut matches = contents
        .lines()
        .filter(|line| keyword_score(line, &keywords) > 0)
        .take(MAX_MEMORY_ITEMS)
        .enumerate()
        .map(|(idx, line)| {
            let compact_line = compact_preflight_body(line, PreflightBodyKind::Memory);
            context_item(
                &format!("memory-{}", idx + 1),
                "Curated memory match",
                &compact_line,
                "memory",
                &memory_path.display().to_string(),
                None,
                "curated memory shares terms with the preflight task",
                vec!["memory".to_string()],
            )
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "low_memory_relevance".to_string(),
            message: "Curated memory produced no keyword-relevant preflight matches.".to_string(),
        });
        matches.push(context_item(
            "memory-empty-1",
            "No relevant curated memory found",
            "Preflight searched curated memory but found no keyword-relevant records.",
            "memory",
            &memory_path.display().to_string(),
            None,
            "records memory-source coverage and explains reduced confidence",
            vec!["memory".to_string(), "degraded".to_string()],
        ));
    }
    packet.sections.push(context_section(
        "memory",
        "Project Memory",
        "Relevant explicit project memory or memory-source warning.",
        matches,
    ));
}

fn add_autoresearch_section(packet: &mut ContextPacket, task: &str, targets: &[String]) -> usize {
    add_autoresearch_to_packet(
        packet,
        AutoresearchPacketBridgeOptions {
            task,
            targets,
            limit: MAX_MEMORY_ITEMS,
            unavailable_message: "No persisted autoresearch store was available for preflight.",
        },
    )
}

fn add_code_section(
    packet: &mut ContextPacket,
    workspace: &Path,
    targets: &[String],
    code_heavy: bool,
) {
    let mut items = Vec::new();
    for target in targets.iter().take(MAX_CODE_ITEMS) {
        let path = workspace.join(target);
        if path.is_file() {
            if let Ok(body) = read_file_excerpt_with_kind(&path, PreflightBodyKind::Code) {
                items.push(context_item(
                    &format!("code-{}", items.len() + 1),
                    target,
                    &body,
                    "file",
                    target,
                    Some(target.clone()),
                    "explicit or inferred target file should be inspected before editing",
                    vec!["code".to_string()],
                ));
            }
        } else if path.is_dir() {
            items.push(context_item(
                &format!("code-{}", items.len() + 1),
                target,
                &summarize_directory(&path),
                "directory",
                target,
                Some(target.clone()),
                "explicit or inferred target directory scopes the implementation",
                vec!["code".to_string()],
            ));
        }
    }
    if items.is_empty() && !targets.is_empty() {
        let target_keywords = targets
            .iter()
            .flat_map(|target| keywords(target))
            .collect::<Vec<_>>();
        items.extend(fallback_code_search(workspace, &target_keywords));
    }
    if items.is_empty() && code_heavy {
        let fallback = fallback_code_search(workspace, &keywords(&packet.query));
        items.extend(fallback);
    }
    if items.is_empty() {
        if code_heavy {
            packet.warnings.push(ContextWarning {
                severity: "warning".to_string(),
                code: "code_context_unavailable".to_string(),
                message: "Preflight classified this as code-heavy but found no target files or fallback code hits.".to_string(),
            });
        }
        return;
    }
    packet.sections.push(code_impact_section(items));
}

fn add_impact_section(packet: &mut ContextPacket, workspace: &Path, targets: &[String]) {
    let Some(primary_target) = targets.first() else {
        return;
    };
    let report = build_impact_report(workspace, primary_target, true, 2);
    let status = match report.status {
        ImpactStatus::Ok => "ok",
        ImpactStatus::Degraded => "degraded",
    };
    let source = match report.source {
        ImpactSource::GitNexus => "gitnexus",
        ImpactSource::Local => "local",
        ImpactSource::Degraded => "degraded",
    };
    let mut body = format!(
        "Target: {}\nStatus: {status}\nSource: {source}\nSummary: {}",
        report.target, report.summary
    );
    if !report.affected_files.is_empty() {
        body.push_str("\nAffected files:");
        for file in report.affected_files.iter().take(8) {
            let _ = write!(body, "\n- {file}");
        }
    }
    if !report.validation_commands.is_empty() {
        body.push_str("\nValidation commands:");
        for command in report.validation_commands.iter().take(5) {
            let _ = write!(body, "\n- {command}");
        }
    }
    if !report.warnings.is_empty() {
        body.push_str("\nWarnings:");
        for warning in report.warnings.iter().take(3) {
            let _ = write!(body, "\n- {warning}");
        }
    }

    packet.sections.push(context_section(
        "impact",
        "Impact Context",
        "GitNexus-backed or degraded local blast-radius context for preflight targets.",
        vec![context_item(
            "impact-1",
            primary_target,
            &body,
            source,
            &format!("impact:{primary_target}"),
            Some(primary_target.clone()),
            "impact context identifies likely blast radius and validation before editing",
            vec!["impact".to_string()],
        )],
    ));
    if report.status == ImpactStatus::Degraded {
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "impact_degraded".to_string(),
            message: format!(
                "Impact analysis for {primary_target} degraded; GitNexus context was unavailable or incomplete."
            ),
        });
    }
}

fn add_validation_section(packet: &mut ContextPacket, code_heavy: bool, targets: &[String]) {
    let commands = infer_validation_commands(code_heavy, targets);
    let items = commands
        .iter()
        .enumerate()
        .map(|(idx, command)| {
            context_item(
                &format!("validation-{}", idx + 1),
                &command.command,
                &format!(
                    "Reason: {}\nRequired: {}\nExpected signal: {}",
                    command.reason, command.required, command.expected_signal
                ),
                "validation",
                "preflight-validation-policy",
                None,
                "validation commands prevent context-only research from becoming unverified implementation",
                vec!["validation".to_string()],
            )
        })
        .collect();
    packet.sections.push(context_section(
        "validation",
        "Suggested Validation Commands",
        "Commands an agent should run after using this research.",
        items,
    ));
}

fn context_item(
    id: &str,
    title: &str,
    body: &str,
    source_kind: &str,
    source_uri: &str,
    repo_path: Option<String>,
    selected_reason: &str,
    tags: Vec<String>,
) -> ContextItem {
    cited_item(
        id,
        title,
        body,
        repo_source(source_kind, source_uri, repo_path),
        selected_reason,
        tags,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidationCommand {
    command: String,
    reason: String,
    required: bool,
    expected_signal: String,
}

fn infer_validation_commands(code_heavy: bool, targets: &[String]) -> Vec<ValidationCommand> {
    let mut commands = vec![ValidationCommand {
        command: "git diff --check".to_string(),
        reason: "detect whitespace and patch hygiene issues before handoff".to_string(),
        required: true,
        expected_signal: "no diff formatting errors".to_string(),
    }];
    if code_heavy {
        commands.push(ValidationCommand {
            command: "cargo test --workspace --all-targets".to_string(),
            reason: "verify all Rust crates and binary targets after code-heavy changes"
                .to_string(),
            required: true,
            expected_signal: "all tests pass".to_string(),
        });
        commands.push(ValidationCommand {
            command: "cargo clippy --workspace --all-targets -- -D warnings".to_string(),
            reason: "production-readiness gate for Rust lint debt".to_string(),
            required: true,
            expected_signal: "no clippy warnings or documented baseline debt".to_string(),
        });
    }
    if targets
        .iter()
        .any(|target| target.contains("context_packet") || target.contains("query"))
    {
        commands.push(ValidationCommand {
            command: "./target/debug/layers query --no-audit --json \"What should I know before editing src/cmd/query.rs?\" | python3 -m json.tool >/dev/null".to_string(),
            reason: "ContextPacket and query changes must keep machine-readable JSON valid".to_string(),
            required: true,
            expected_signal: "JSON parses successfully".to_string(),
        });
    }
    commands
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResearchQuality {
    confidence: String,
    passes_minimum_bar: bool,
}

impl ResearchQuality {
    #[allow(clippy::fn_params_excessive_bools)]
    fn new(code_heavy: bool, has_memory: bool, has_code: bool, low_memory_relevance: bool) -> Self {
        // Low-relevance or memory-only context must never be reported as high confidence.
        let confidence = if !has_memory && !has_code {
            // No useful context at all.
            "low"
        } else if (code_heavy || low_memory_relevance) && !has_code {
            // Code-heavy task without code, or any task with only low-relevance memory
            // and no code to compensate, cannot claim medium or high confidence.
            "low"
        } else if low_memory_relevance {
            // Has code but memory is weak; cap at medium.
            "medium"
        } else if has_memory && has_code {
            "high"
        } else {
            "medium"
        };
        let passes_minimum_bar = confidence != "low";
        Self {
            confidence: confidence.to_string(),
            passes_minimum_bar,
        }
    }
}

fn is_code_heavy_task(task: &str, targets: &[String]) -> bool {
    let lower = task.to_lowercase();
    let code_terms = [
        "code",
        "clippy",
        "test",
        "tests",
        "rust",
        "refactor",
        "build",
        "mcp",
        "validate",
        "implementation",
        "feature",
        "function",
        "module",
        "crate",
        "preflight",
    ];
    targets.iter().any(|target| looks_like_path(target))
        || code_terms.iter().any(|term| lower.contains(term))
}

fn infer_targets(task: &str, explicit_targets: &[String], workspace: &Path) -> Vec<String> {
    let mut targets = explicit_targets.to_vec();
    for token in task.split_whitespace() {
        let cleaned = token.trim_matches(|ch: char| {
            matches!(ch, ',' | '.' | ':' | ';' | ')' | '(' | '`' | '"' | '\'')
        });
        if looks_like_path(cleaned) && !targets.iter().any(|target| target == cleaned) {
            targets.push(cleaned.to_string());
        }
    }
    if targets.is_empty() && task.to_lowercase().contains("preflight") {
        for fallback in ["src/cmd/preflight.rs", "src/main.rs", "src/cmd/mod.rs"] {
            if workspace.join(fallback).exists() || fallback == "src/cmd/preflight.rs" {
                targets.push(fallback.to_string());
            }
        }
    }
    targets
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/')
        || has_source_extension(value)
        || value.starts_with("crates/")
        || value.starts_with("src/")
        || value.starts_with("docs/")
}

fn has_source_extension(value: &str) -> bool {
    Path::new(value).extension().is_some_and(|ext| {
        ext.to_str()
            .map(str::to_ascii_lowercase)
            .is_some_and(|ext| matches!(ext.as_str(), "rs" | "md" | "toml"))
    })
}

fn keywords(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .filter_map(|word| {
            let lower = word.to_lowercase();
            (lower.len() >= 4).then_some(lower)
        })
        .collect()
}

fn keyword_score(text: &str, keywords: &[String]) -> usize {
    let lower = text.to_lowercase();
    keywords
        .iter()
        .filter(|keyword| lower.contains(keyword.as_str()))
        .count()
}

fn fallback_code_search(workspace: &Path, keywords: &[String]) -> Vec<ContextItem> {
    let files = git_output(workspace, &["ls-files"]).unwrap_or_default();
    files
        .lines()
        .filter(|file| has_source_extension(file))
        .filter_map(|file| {
            let path = workspace.join(file);
            let content = std::fs::read_to_string(&path).ok()?;
            (keyword_score(&content, keywords) > 0).then(|| {
                let excerpt = compact_preflight_body(&content, PreflightBodyKind::Code);
                context_item(
                    file,
                    file,
                    &excerpt,
                    "file_search",
                    file,
                    Some(file.to_string()),
                    "fallback code search found query terms in this tracked file",
                    vec!["code".to_string(), "fallback".to_string()],
                )
            })
        })
        .take(MAX_CODE_ITEMS)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreflightBodyKind {
    Code,
    Memory,
    Policy,
}

fn compact_preflight_body(body: &str, kind: PreflightBodyKind) -> String {
    let (max_lines, max_chars) = match kind {
        PreflightBodyKind::Code => (COMPACT_CODE_EXCERPT_LINES, COMPACT_CODE_EXCERPT_CHARS),
        PreflightBodyKind::Memory => (usize::MAX, COMPACT_MEMORY_ITEM_CHARS),
        PreflightBodyKind::Policy => (24, 1_200),
    };
    let mut out = String::new();
    let mut truncated_lines = 0usize;
    let mut truncated_chars = 0usize;
    for (idx, line) in body.lines().enumerate() {
        if idx >= max_lines {
            truncated_lines += 1;
            truncated_chars += line.len();
            continue;
        }
        let separator_len = usize::from(!out.is_empty());
        let available = max_chars.saturating_sub(out.len().saturating_add(separator_len));
        if line.len() > available {
            let prefix = utf8_prefix_at_most(line, available);
            if !prefix.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(prefix);
            }
            truncated_lines += 1;
            truncated_chars += line.len().saturating_sub(prefix.len());
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    if truncated_lines > 0 || truncated_chars > 0 {
        if !out.is_empty() {
            out.push('\n');
        }
        write!(
            out,
            "[truncated: omitted {truncated_lines} lines and at least {truncated_chars} chars to fit preflight budget]"
        )
        .expect("writing to a String cannot fail");
    }
    out
}

fn utf8_prefix_at_most(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn read_file_excerpt(path: &Path) -> Result<String> {
    read_file_excerpt_with_kind(path, PreflightBodyKind::Policy)
}

fn read_file_excerpt_with_kind(path: &Path, kind: PreflightBodyKind) -> Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read target {}", path.display()))?;
    Ok(compact_preflight_body(&content, kind))
}

fn summarize_directory(path: &Path) -> String {
    let Ok(entries) = std::fs::read_dir(path) else {
        return "Directory could not be read.".to_string();
    };
    entries
        .filter_map(Result::ok)
        .take(30)
        .map(|entry| entry.path())
        .filter_map(|path: PathBuf| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn estimate_packet_words(packet: &ContextPacket) -> usize {
    packet
        .sections
        .iter()
        .flat_map(|section| section.items.iter())
        .map(|item| item.token_estimate)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::autoresearch::{
        AutoresearchCommands, ProfileCommands, SourceCommands, handle_autoresearch,
    };
    use crate::context_packet_compiler::WorkspaceState;
    use crate::test_support::TestWorkspace;

    #[test]
    fn code_heavy_tasks_require_code_context() {
        assert!(is_code_heavy_task(
            "fix clippy failures in src/cmd/query.rs and add tests",
            &["src/cmd/query.rs".to_string()]
        ));
    }

    #[test]
    fn low_relevance_memory_only_cannot_be_high_confidence() {
        let quality = ResearchQuality::new(true, false, false, true);
        assert_eq!(quality.confidence, "low");
        assert!(!quality.passes_minimum_bar);
    }

    #[test]
    fn low_relevance_memory_with_code_is_not_high_confidence() {
        let quality = ResearchQuality::new(true, true, true, true);
        assert_eq!(quality.confidence, "medium");
        assert!(quality.passes_minimum_bar);
    }

    #[test]
    fn validation_commands_include_rust_release_gate_for_code_tasks() {
        let commands = infer_validation_commands(true, &["src/cmd/query.rs".to_string()]);
        assert!(
            commands
                .iter()
                .any(|cmd| cmd.command == "cargo test --workspace --all-targets")
        );
        assert!(
            commands
                .iter()
                .any(|cmd| cmd.command == "cargo clippy --workspace --all-targets -- -D warnings")
        );
        assert!(commands.iter().any(|cmd| cmd.command == "git diff --check"));
    }

    #[test]
    fn workspace_state_reports_dirty_warning() {
        let state = WorkspaceState {
            branch: Some("main".to_string()),
            head: Some("abc123".to_string()),
            dirty: true,
            changed_files: vec!["src/main.rs".to_string()],
            untracked_files: vec!["src/cmd/preflight.rs".to_string()],
            changed_total: 1,
            untracked_total: 1,
            truncated: false,
        };
        let warnings = state.warnings();
        assert!(
            warnings
                .iter()
                .any(|warning| warning.code == "dirty_worktree")
        );
    }

    #[test]
    fn preflight_packet_uses_preflight_route_and_id() {
        let args = PreflightArgs {
            task: "fix src/main.rs".to_string(),
            targets: Vec::new(),
            json: true,
            agent_prompt: false,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();

        assert_eq!(packet.route, "preflight");
        assert_eq!(packet.provenance.surface, "preflight");
        assert!(packet.id.starts_with("preflight-"));
        assert_eq!(packet.task, "fix src/main.rs");
    }

    #[test]
    fn targeted_preflight_confidence_and_injection_policy_are_consistent() {
        let ws = TestWorkspace::new("preflight-confidence-policy");
        std::fs::create_dir_all(ws.root().join("src/cmd")).unwrap();
        std::fs::write(
            ws.root().join("src/cmd/preflight.rs"),
            "fn build_preflight_packet() { /* grounded test target */ }\n",
        )
        .unwrap();
        let args = PreflightArgs {
            task: "fix preflight packet confidence in src/cmd/preflight.rs".to_string(),
            targets: vec!["src/cmd/preflight.rs".to_string()],
            json: true,
            agent_prompt: false,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();
        let policy_warning = packet
            .warnings
            .iter()
            .find(|warning| warning.code == "injection_policy")
            .expect("preflight should include injection policy warning");

        assert_ne!(packet.confidence, "low");
        assert!(!packet.low_confidence_fallback);
        assert!(packet.retrieval.fallback_reason.is_none());
        assert!(
            !policy_warning
                .message
                .contains("low-confidence or fallback-derived"),
            "well-grounded targeted preflight must not cite fallback-derived reasons: {}",
            policy_warning.message
        );
    }

    #[test]
    fn preflight_rendering_does_not_call_it_autoresearch() {
        let args = PreflightArgs {
            task: "fix src/main.rs".to_string(),
            targets: Vec::new(),
            json: false,
            agent_prompt: false,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();
        let rendered = packet.to_markdown();

        assert!(rendered.contains("Route: preflight"));
        assert!(
            packet
                .sections
                .iter()
                .any(|section| section.title.contains("Preflight"))
        );
        assert!(!packet.route.contains("autoresearch"));
        assert!(!packet.why_retrieved.contains("Autoresearch"));
    }

    #[test]
    fn preflight_agent_prompt_uses_compact_objective_brief() {
        let ws = TestWorkspace::new("preflight-agent-prompt-compact");
        std::fs::create_dir_all(ws.root().join("src/cmd")).unwrap();
        std::fs::write(
            ws.root().join("src/cmd/preflight.rs"),
            "fn build_preflight_packet() { /* grounded test target */ }\n",
        )
        .unwrap();
        let args = PreflightArgs {
            task: "fix src/cmd/preflight.rs".to_string(),
            targets: vec!["src/cmd/preflight.rs".to_string()],
            json: false,
            agent_prompt: true,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();
        let rendered = render_preflight_agent_prompt(&packet);

        assert!(rendered.starts_with("# Objective Brief"));
        assert!(rendered.contains("## Handoff Expectations"));
        assert!(!rendered.contains("<layers_context>"));
        assert!(!rendered.contains("grounded test target"));
    }

    #[test]
    fn targeted_preflight_includes_impact_section() {
        let ws = TestWorkspace::new("preflight-impact-section");
        std::fs::create_dir_all(ws.root().join("src/cmd")).unwrap();
        std::fs::write(ws.root().join("src/cmd/preflight.rs"), "pub fn demo() {}\n").unwrap();
        let args = PreflightArgs {
            task: "fix src/cmd/preflight.rs".to_string(),
            targets: vec!["src/cmd/preflight.rs".to_string()],
            json: true,
            agent_prompt: false,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();
        let section = packet
            .sections
            .iter()
            .find(|section| section.id == "impact")
            .expect("targeted preflight should include impact context");

        assert!(
            section.items[0]
                .body
                .contains("Target: src/cmd/preflight.rs")
        );
        assert!(section.items[0].body.contains("Validation commands:"));
    }

    #[test]
    fn preflight_includes_repo_owned_context_policy() {
        let ws = TestWorkspace::new("preflight-context-policy");
        std::fs::write(
            ws.root().join("LAYERS.md"),
            "# Layers Context Policy\n\nPacket budget: 1200 words.\n\nValidation: run cargo test -p layers cmd::preflight.\n\nDanger: do not expand stable MCP into generic runtime tools.\n",
        )
        .unwrap();
        let args = PreflightArgs {
            task: "prepare context before editing src/cmd/preflight.rs".to_string(),
            targets: vec!["src/cmd/preflight.rs".to_string()],
            json: true,
            agent_prompt: false,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();
        let section = packet
            .sections
            .iter()
            .find(|section| section.id == "context_policy")
            .expect("repo-owned context policy should become packet context");

        assert_eq!(section.items[0].source.kind, "context_policy");
        assert_eq!(
            section.items[0].source.repo_path.as_deref(),
            Some("LAYERS.md")
        );
        assert!(section.items[0].body.contains("Packet budget: 1200 words"));
        assert!(section.items[0].body.contains("stable MCP"));
        assert!(
            packet
                .selection_trace
                .iter()
                .any(|trace| trace.item_id == "context-policy-1")
        );
    }

    #[test]
    fn preflight_bridges_autoresearch_findings_into_context_packet() {
        let _ws = TestWorkspace::new("preflight-autoresearch-bridge");
        seed_autoresearch_store("preflight");
        let args = PreflightArgs {
            task: "fill context compiler autoresearch gap".to_string(),
            targets: vec!["src/cmd/autoresearch.rs".to_string()],
            json: true,
            agent_prompt: false,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();
        let section = packet
            .sections
            .iter()
            .find(|section| section.id == "autoresearch")
            .expect("preflight should include task-matched autoresearch findings");

        assert_eq!(
            packet.scores["preflight"]["autoresearch_findings"].as_u64(),
            Some(1)
        );
        assert!(packet.scores.get("packet_quality").is_some());
        assert!(packet.scores.get("injection_recommendation").is_some());
        assert!(
            packet.evidence.is_empty(),
            "preflight JSON should not duplicate section bodies in the legacy evidence field"
        );
        assert!(
            section.items[0]
                .body
                .contains("Context compiler autoresearch gap")
        );
        assert!(section.items[0].body.contains("Provenance:"));
        assert_eq!(section.items[0].source.kind, "autoresearch");
        assert!(
            packet
                .selection_trace
                .iter()
                .any(|trace| trace.item_id == "autoresearch-1")
        );
    }

    fn seed_autoresearch_store(prefix: &str) {
        handle_autoresearch(&AutoresearchCommands::Source {
            command: SourceCommands::Add {
                url: format!("file:///{prefix}/context-compiler-autoresearch-gap.md"),
                title: Some("Context compiler autoresearch gap resolution".to_string()),
                source_type: "article".to_string(),
            },
        })
        .unwrap();
        handle_autoresearch(&AutoresearchCommands::Profile {
            command: ProfileCommands::Create {
                name: "Context compiler".to_string(),
                keywords: "context,compiler,autoresearch,gap".to_string(),
                negative_keywords: None,
                score_threshold: Some(1.0),
                max_llm_calls: Some(0),
                json: true,
            },
        })
        .unwrap();
        handle_autoresearch(&AutoresearchCommands::ScanOnce {
            profile_id: None,
            json: true,
        })
        .unwrap();
    }

    #[test]
    fn preflight_compacts_long_memory_and_code_excerpts() {
        let long_text = (0..80)
            .map(|idx| {
                format!(
                    "line {idx}: this is detailed implementation context that should be trimmed"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let compact_code = compact_preflight_body(&long_text, PreflightBodyKind::Code);
        let compact_memory = compact_preflight_body(&long_text, PreflightBodyKind::Memory);

        assert!(compact_code.lines().count() <= COMPACT_CODE_EXCERPT_LINES + 1);
        assert!(compact_code.len() <= COMPACT_CODE_EXCERPT_CHARS + 80);
        assert!(compact_code.contains("[truncated:"));
        assert!(compact_memory.len() <= COMPACT_MEMORY_ITEM_CHARS + 80);
        assert!(compact_memory.contains("[truncated:"));
    }

    #[test]
    fn preflight_compaction_preserves_prefix_for_single_long_lines() {
        let long_memory = format!(
            "critical decision: {}",
            "retain this exact evidence phrase ".repeat(20)
        );
        let compact_memory = compact_preflight_body(&long_memory, PreflightBodyKind::Memory);

        assert!(compact_memory.starts_with("critical decision: retain this exact evidence"));
        assert!(compact_memory.contains("[truncated:"));
        assert!(compact_memory.len() <= COMPACT_MEMORY_ITEM_CHARS + 80);

        let long_code = format!("fn important_symbol() {{ {} }}", "do_work(); ".repeat(120));
        let compact_code = compact_preflight_body(&long_code, PreflightBodyKind::Code);

        assert!(compact_code.starts_with("fn important_symbol()"));
        assert!(compact_code.contains("[truncated:"));
        assert!(compact_code.len() <= COMPACT_CODE_EXCERPT_CHARS + 80);
    }

    #[test]
    fn preflight_compaction_keeps_utf8_boundaries() {
        let long_text = format!("重要な判断: {}", "境界".repeat(200));
        let compact = compact_preflight_body(&long_text, PreflightBodyKind::Memory);

        assert!(compact.starts_with("重要な判断:"));
        assert!(compact.contains("[truncated:"));
    }

    #[test]
    fn strict_minimum_bar_requires_memory_section() {
        let mut packet = ContextPacket::new(
            "test".to_string(),
            "layers".to_string(),
            "fix src/main.rs".to_string(),
            chrono::Utc::now(),
        );
        packet.scores = json!({ "code_heavy": true });
        packet.sections.push(context_section(
            "workspace",
            "Workspace State",
            "Current Git/worktree context that can affect safe edits.",
            Vec::new(),
        ));
        packet.sections.push(code_impact_section(Vec::new()));
        packet.sections.push(context_section(
            "validation",
            "Validation",
            "Validation commands required after using this packet.",
            Vec::new(),
        ));

        assert!(!minimum_bar_passes(&packet));
    }

    // ── Regression: low-relevance / memory-only cannot be high confidence ─────

    #[test]
    fn regression_low_relevance_memory_only_is_low_confidence() {
        // Non-code-heavy task with only low-relevance memory and no code.
        let quality = ResearchQuality::new(false, true, false, true);
        assert_eq!(
            quality.confidence, "low",
            "low-relevance memory-only context must be low confidence"
        );
        assert!(
            !quality.passes_minimum_bar,
            "low-relevance memory-only must not pass minimum bar"
        );
    }

    #[test]
    fn regression_no_memory_no_code_is_low_confidence() {
        // Any task with neither memory nor code context.
        let quality = ResearchQuality::new(false, false, false, false);
        assert_eq!(
            quality.confidence, "low",
            "no memory and no code must be low confidence"
        );
        assert!(
            !quality.passes_minimum_bar,
            "empty context must not pass minimum bar"
        );
    }

    #[test]
    fn regression_strict_rejects_low_confidence_packet() {
        // Build a packet that has all required sections but low confidence.
        let mut packet = ContextPacket::new(
            "test".to_string(),
            "layers".to_string(),
            "orientation task".to_string(),
            chrono::Utc::now(),
        );
        packet.confidence = "low".to_string();
        packet.scores = json!({ "code_heavy": false });
        packet.sections.push(context_section(
            "workspace",
            "Workspace State",
            "Current Git/worktree context.",
            Vec::new(),
        ));
        packet.sections.push(context_section(
            "memory",
            "Project Memory",
            "Relevant explicit project memory.",
            Vec::new(),
        ));
        packet.sections.push(context_section(
            "validation",
            "Validation",
            "Validation commands.",
            Vec::new(),
        ));

        assert!(
            !minimum_bar_passes(&packet),
            "strict minimum bar must reject low-confidence packets even with all sections present"
        );
    }

    #[test]
    fn regression_high_confidence_still_requires_memory_and_code() {
        let quality = ResearchQuality::new(true, true, true, false);
        assert_eq!(quality.confidence, "high");
        assert!(quality.passes_minimum_bar);
    }

    #[test]
    fn add_specific_instructions_extracts_imperative_constraints() {
        let ws = TestWorkspace::new("specific-instructions-extract");
        let memory_path = ws.root().join("memoryport").join("curated-memory.jsonl");
        let lines = [
            r#"{"entity":"constraint","id":"c1","payload":{"summary":"Do not modify the router without running tests"},"project":"layers","tags":["router"],"confidence":"high"}"#,
            r#"{"entity":"constraint","id":"c2","payload":{"summary":"Always validate packets before sending"},"project":"layers","tags":["packet"],"confidence":"high"}"#,
            r#"{"entity":"decision","id":"d1","payload":{"summary":"Do not change this decision"},"project":"layers","tags":["decision"]}"#,
            r#"{"entity":"constraint","id":"c3","payload":{"summary":"Never skip validation on router input"},"project":"layers","tags":["router","validation"],"confidence":"medium"}"#,
            r#"{"entity":"pitfall","id":"p1","payload":{"summary":"Avoid using unwrap in router handlers"},"project":"layers","tags":["router"],"confidence":"high"}"#,
            r#"{"entity":"constraint","id":"c4","payload":{"summary":"Must ensure test coverage for router changes"},"project":"layers","tags":["router"],"confidence":"low"}"#,
        ];
        std::fs::write(&memory_path, lines.join("\n")).unwrap();

        let mut packet = ContextPacket::new(
            "test".to_string(),
            "layers".to_string(),
            "fix router tests".to_string(),
            chrono::Utc::now(),
        );
        add_specific_instructions(&mut packet, "fix router tests");

        // Should find constraint c1 (Do not...), constraint c3 (Never...), and pitfall p1 (Avoid...)
        // c4 has low confidence so excluded. c2 doesn't match "router" keyword. d1 is a decision.
        assert!(
            packet.specific_instructions.len() <= 3,
            "at most 3 instructions"
        );
        assert!(
            !packet.specific_instructions.is_empty(),
            "should find matching imperative constraints"
        );
        for instr in &packet.specific_instructions {
            assert!(
                matches!(instr.kind.as_str(), "constraint" | "pitfall" | "learning"),
                "only constraint/pitfall/learning kinds allowed: {}",
                instr.kind
            );
            assert!(
                matches!(instr.confidence.as_str(), "medium" | "high"),
                "only medium/high confidence allowed: {}",
                instr.confidence
            );
        }
    }

    #[test]
    fn add_specific_instructions_returns_empty_when_no_matches() {
        let ws = TestWorkspace::new("specific-instructions-empty");
        let memory_path = ws.root().join("memoryport").join("curated-memory.jsonl");
        let lines = [
            r#"{"entity":"decision","id":"d1","payload":{"summary":"Use Rust for systems"},"project":"layers","tags":["rust"]}"#,
        ];
        std::fs::write(&memory_path, lines.join("\n")).unwrap();

        let mut packet = ContextPacket::new(
            "test".to_string(),
            "layers".to_string(),
            "something completely unrelated like quantum computing".to_string(),
            chrono::Utc::now(),
        );
        add_specific_instructions(
            &mut packet,
            "something completely unrelated like quantum computing",
        );

        assert!(
            packet.specific_instructions.is_empty(),
            "no matching instructions expected for unrelated task"
        );
    }

    #[test]
    fn add_specific_instructions_handles_missing_file() {
        let ws = TestWorkspace::new("specific-instructions-missing");
        // Do not create curated-memory.jsonl
        let _ = ws;

        let mut packet = ContextPacket::new(
            "test".to_string(),
            "layers".to_string(),
            "fix something".to_string(),
            chrono::Utc::now(),
        );
        add_specific_instructions(&mut packet, "fix something");

        assert!(
            packet.specific_instructions.is_empty(),
            "missing file should produce no instructions"
        );
    }
}
