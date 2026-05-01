use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail};
use clap::{Subcommand, ValueEnum};
use layers_core::context_packet::{CONTEXT_PACKET_SCHEMA_VERSION, ContextPacket};
use layers_core::{PacketQualityReport, TaskSpec};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PacketRenderFormat {
    Markdown,
    AgentPrompt,
    ObjectiveBrief,
    Json,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PacketCommands {
    /// Validate a `ContextPacket` artifact from disk.
    Validate {
        /// Path to a `ContextPacket` JSON artifact.
        path: PathBuf,
        /// Fail on packet quality issues that are warnings by default.
        #[arg(long)]
        strict: bool,
        /// Output a structured JSON validation report.
        #[arg(long)]
        json: bool,
    },
    /// Inspect a `ContextPacket` artifact without recompiling it.
    Inspect {
        /// Path to a `ContextPacket` JSON artifact.
        path: PathBuf,
        /// Output a structured JSON inspection report.
        #[arg(long)]
        json: bool,
    },
    /// Render a `ContextPacket` artifact without recompiling it.
    Render {
        /// Path to a `ContextPacket` JSON artifact.
        path: PathBuf,
        /// Output format for the rendered packet.
        #[arg(long, value_enum)]
        format: PacketRenderFormat,
    },
    /// Compare two `ContextPacket` artifacts semantically.
    Diff {
        /// Older `ContextPacket` JSON artifact.
        old: PathBuf,
        /// Newer `ContextPacket` JSON artifact.
        new: PathBuf,
        /// Output a structured JSON diff report.
        #[arg(long)]
        json: bool,
    },
    /// Grade a `ContextPacket` against a workflow task spec.
    Grade {
        /// Path to a `ContextPacket` JSON artifact.
        path: PathBuf,
        /// Path to a workflow task spec JSON artifact.
        #[arg(long)]
        task: PathBuf,
        /// Output a structured JSON quality report.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn handle_packet(command: &PacketCommands) -> Result<()> {
    match command {
        PacketCommands::Validate { path, strict, json } => validate_packet(path, *strict, *json),
        PacketCommands::Inspect { path, json } => inspect_packet(path, *json),
        PacketCommands::Render { path, format } => render_packet(path, *format),
        PacketCommands::Diff { old, new, json } => diff_packet(old, new, *json),
        PacketCommands::Grade { path, task, json } => grade_packet(path, task, *json),
    }
}

fn inspect_packet(path: &Path, json: bool) -> Result<()> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read ContextPacket artifact {}", path.display()))?;
    let output = inspect_packet_text(&text, json)?;
    println!("{output}");
    Ok(())
}

fn render_packet(path: &Path, format: PacketRenderFormat) -> Result<()> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read ContextPacket artifact {}", path.display()))?;
    let output = render_packet_text(&text, format)?;
    println!("{output}");
    Ok(())
}

fn diff_packet(old: &Path, new: &Path, json: bool) -> Result<()> {
    let old_text = fs::read_to_string(old).with_context(|| {
        format!(
            "failed to read old ContextPacket artifact {}",
            old.display()
        )
    })?;
    let new_text = fs::read_to_string(new).with_context(|| {
        format!(
            "failed to read new ContextPacket artifact {}",
            new.display()
        )
    })?;
    let output = diff_packet_text(&old_text, &new_text, json)?;
    println!("{output}");
    Ok(())
}

fn grade_packet(path: &Path, task_path: &Path, json: bool) -> Result<()> {
    let packet_text = fs::read_to_string(path)
        .with_context(|| format!("failed to read ContextPacket artifact {}", path.display()))?;
    let task_text = fs::read_to_string(task_path)
        .with_context(|| format!("failed to read task spec artifact {}", task_path.display()))?;
    let output = grade_packet_text(&packet_text, &task_text, json)?;
    println!("{output}");
    Ok(())
}

fn validate_packet(path: &Path, strict: bool, json: bool) -> Result<()> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read ContextPacket artifact {}", path.display()))?;
    let report = validate_packet_text(&text, strict)?;
    print_validation_report(&report, json)?;

    if report.is_valid() {
        Ok(())
    } else {
        bail!("ContextPacket validation failed")
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct PacketValidationReport {
    valid: bool,
    strict: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl PacketValidationReport {
    #[must_use]
    const fn is_valid(&self) -> bool {
        self.valid
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct PacketInspectionReport {
    schema_version: u32,
    id: String,
    workspace_id: String,
    query: String,
    created_at: String,
    git_ref: Option<String>,
    route: String,
    confidence: String,
    budget: PacketBudgetInspection,
    provenance: PacketProvenanceInspection,
    section_count: usize,
    item_count: usize,
    warning_count: usize,
    degraded: bool,
    low_confidence_fallback: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct PacketDiffReport {
    old_id: String,
    new_id: String,
    metadata_changes: Vec<String>,
    removed_sections: Vec<String>,
    added_sections: Vec<String>,
    changed_sections: Vec<String>,
    removed_items: Vec<String>,
    added_items: Vec<String>,
    changed_items: Vec<String>,
    warning_changes: Vec<String>,
    summary: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct PacketBudgetInspection {
    used_units: usize,
    max_units: usize,
    unit: String,
    truncated: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct PacketProvenanceInspection {
    compiler: String,
    compiler_version: String,
    surface: String,
    generated_at: String,
    source_adapters: Vec<String>,
}

fn print_validation_report(report: &PacketValidationReport, json: bool) -> Result<()> {
    if report.is_valid() {
        println!("ContextPacket validation passed");
    } else {
        println!("ContextPacket validation failed");
    }

    for warning in &report.warnings {
        println!("warning: {warning}");
    }
    for error in &report.errors {
        println!("error: {error}");
    }

    if json {
        let serialized = serde_json::to_string_pretty(report)
            .context("failed to serialize packet validation report")?;
        println!("{serialized}");
    }

    Ok(())
}

fn validate_packet_text(text: &str, strict: bool) -> Result<PacketValidationReport> {
    let value: Value = serde_json::from_str(text).context("invalid ContextPacket JSON")?;

    let mut pre_deserialization_errors = Vec::new();
    validate_secret_like_values(&value, &mut pre_deserialization_errors);
    if !pre_deserialization_errors.is_empty() {
        return Ok(PacketValidationReport {
            valid: false,
            strict,
            errors: pre_deserialization_errors,
            warnings: Vec::new(),
        });
    }

    let packet: ContextPacket = serde_json::from_value(value.clone())
        .map_err(|_| anyhow::anyhow!("JSON did not match the ContextPacket v2 artifact shape"))?;

    Ok(validate_packet_value(&value, &packet, strict))
}

fn inspect_packet_text(text: &str, json: bool) -> Result<String> {
    let value: Value = serde_json::from_str(text).context("invalid ContextPacket JSON")?;
    let mut pre_deserialization_errors = Vec::new();
    validate_secret_like_values(&value, &mut pre_deserialization_errors);
    if !pre_deserialization_errors.is_empty() {
        bail!(
            "ContextPacket validation failed: {}",
            pre_deserialization_errors.join("; ")
        );
    }

    let packet: ContextPacket = serde_json::from_value(value.clone())
        .map_err(|_| anyhow::anyhow!("JSON did not match the ContextPacket v2 artifact shape"))?;
    let validation = validate_packet_value(&value, &packet, false);
    if !validation.is_valid() {
        bail!(
            "ContextPacket validation failed: {}",
            validation.errors.join("; ")
        );
    }

    let inspection = inspect_context_packet(&packet);
    if json {
        serde_json::to_string_pretty(&inspection)
            .context("failed to serialize packet inspection report")
    } else {
        Ok(format_inspection_report(&inspection))
    }
}

fn render_packet_text(text: &str, format: PacketRenderFormat) -> Result<String> {
    let packet = parse_valid_packet_text(text)?;

    match format {
        PacketRenderFormat::Markdown => Ok(packet.to_markdown()),
        PacketRenderFormat::AgentPrompt => Ok(packet.to_agent_prompt()),
        PacketRenderFormat::ObjectiveBrief => Ok(format_objective_brief(&packet)),
        PacketRenderFormat::Json => {
            serde_json::to_string_pretty(&packet).context("failed to serialize ContextPacket JSON")
        }
    }
}

fn format_objective_brief(packet: &ContextPacket) -> String {
    let mut out = String::new();
    out.push_str("# Objective Brief\n\n");
    out.push_str("## Objective\n\n");
    out.push_str(&packet.query);
    out.push_str("\n\n");
    out.push_str("## Context Constraints\n\n");
    let _ = writeln!(out, "- packet: {}", packet.id);
    let _ = writeln!(out, "- workspace: {}", packet.workspace_id);
    let _ = writeln!(out, "- route: {}", packet.route);
    let _ = writeln!(out, "- confidence: {}", packet.confidence);
    let _ = writeln!(
        out,
        "- budget: {}/{} {}{}",
        packet.budget.used_units,
        packet.budget.max_units,
        packet.budget.unit,
        if packet.budget.truncated {
            " (truncated)"
        } else {
            ""
        }
    );
    if let Some(git_ref) = &packet.git_ref {
        let _ = writeln!(out, "- git ref: {git_ref}");
    }
    out.push('\n');

    if !packet.warnings.is_empty() {
        out.push_str("## Warnings\n\n");
        for warning in &packet.warnings {
            let _ = writeln!(
                out,
                "- [{}] {}: {}",
                warning.severity, warning.code, warning.message
            );
        }
        out.push('\n');
    }

    out.push_str("## Cited Context\n\n");
    for section in &packet.sections {
        for item in &section.items {
            let _ = writeln!(out, "- {} / {}", section.title, item.title);
            let _ = writeln!(out, "  source: {}", format_source_citation(item));
            let _ = writeln!(out, "  selected because: {}", item.selected_reason);
            if !item.tags.is_empty() {
                let _ = writeln!(out, "  tags: {}", item.tags.join(", "));
            }
        }
    }
    out.push('\n');

    out.push_str("## Validation Plan\n\n");
    let validation_commands = validation_commands(packet);
    if validation_commands.is_empty() {
        out.push_str("No explicit validation commands were captured in this packet.\n\n");
    } else {
        for command in validation_commands {
            let _ = writeln!(out, "- `{command}`");
        }
        out.push('\n');
    }

    out.push_str("## Handoff Expectations\n\n");
    out.push_str("- Use the cited context above as the task boundary.\n");
    out.push_str("- Do not assume uncited repository facts are current.\n");
    out.push_str("- Preserve packet citations in review notes when they justify edits.\n");
    out.push_str(
        "- If the cited context is insufficient, stop and request an updated ContextPacket or explicit targets.\n",
    );
    out
}

fn format_source_citation(item: &layers_core::context_packet::ContextItem) -> String {
    let mut citation = format!("{} ({})", item.source.uri, item.source.kind);
    if let Some(repo_path) = &item.source.repo_path {
        let _ = write!(citation, " [{repo_path}");
        if let Some(line_range) = &item.source.line_range {
            let _ = write!(citation, ":{line_range}");
        }
        citation.push(']');
    }
    if let Some(commit) = &item.source.commit {
        let _ = write!(citation, " @ {commit}");
    }
    citation
}

fn validation_commands(packet: &ContextPacket) -> Vec<String> {
    packet
        .sections
        .iter()
        .flat_map(|section| section.items.iter())
        .flat_map(|item| item.tags.iter())
        .filter_map(|tag| tag.strip_prefix("validate:"))
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn diff_packet_text(old_text: &str, new_text: &str, json: bool) -> Result<String> {
    let old_packet = parse_valid_packet_text(old_text)?;
    let new_packet = parse_valid_packet_text(new_text)?;
    let report = diff_context_packets(&old_packet, &new_packet)?;

    if json {
        serde_json::to_string_pretty(&report).context("failed to serialize packet diff report")
    } else {
        Ok(format_diff_report(&report))
    }
}

fn parse_valid_packet_text(text: &str) -> Result<ContextPacket> {
    let value: Value = serde_json::from_str(text).context("invalid ContextPacket JSON")?;
    let mut pre_deserialization_errors = Vec::new();
    validate_secret_like_values(&value, &mut pre_deserialization_errors);
    if !pre_deserialization_errors.is_empty() {
        bail!(
            "ContextPacket validation failed: {}",
            pre_deserialization_errors.join("; ")
        );
    }

    let packet: ContextPacket = serde_json::from_value(value.clone())
        .map_err(|_| anyhow::anyhow!("JSON did not match the ContextPacket v2 artifact shape"))?;
    let validation = validate_packet_value(&value, &packet, false);
    if !validation.is_valid() {
        bail!(
            "ContextPacket validation failed: {}",
            validation.errors.join("; ")
        );
    }

    Ok(packet)
}

fn diff_context_packets(old: &ContextPacket, new: &ContextPacket) -> Result<PacketDiffReport> {
    let metadata_changes = diff_packet_metadata(old, new);
    let old_sections = section_map(old)?;
    let new_sections = section_map(new)?;
    let old_items = item_map(old)?;
    let new_items = item_map(new)?;

    let removed_sections = removed_keys(&old_sections, &new_sections);
    let added_sections = added_keys(&old_sections, &new_sections);
    let changed_sections = changed_keys(&old_sections, &new_sections);
    let removed_items = removed_keys(&old_items, &new_items);
    let added_items = added_keys(&old_items, &new_items);
    let changed_items = changed_keys(&old_items, &new_items);
    let warning_changes = diff_warnings(old, new);
    let summary = format!(
        "{} removed, {} added, {} changed",
        removed_items.len(),
        added_items.len(),
        changed_items.len()
    );

    Ok(PacketDiffReport {
        old_id: old.id.clone(),
        new_id: new.id.clone(),
        metadata_changes,
        removed_sections,
        added_sections,
        changed_sections,
        removed_items,
        added_items,
        changed_items,
        warning_changes,
        summary,
    })
}

fn diff_packet_metadata(old: &ContextPacket, new: &ContextPacket) -> Vec<String> {
    let mut changes = Vec::new();
    push_change(
        &mut changes,
        "schema_version",
        old.schema_version,
        new.schema_version,
    );
    push_change(
        &mut changes,
        "workspace_id",
        &old.workspace_id,
        &new.workspace_id,
    );
    push_change(&mut changes, "query", &old.query, &new.query);
    push_change(&mut changes, "git_ref", &old.git_ref, &new.git_ref);
    push_change(&mut changes, "route", &old.route, &new.route);
    push_change(&mut changes, "confidence", &old.confidence, &new.confidence);
    push_change(
        &mut changes,
        "budget.used_units",
        old.budget.used_units,
        new.budget.used_units,
    );
    push_change(
        &mut changes,
        "budget.max_units",
        old.budget.max_units,
        new.budget.max_units,
    );
    push_change(
        &mut changes,
        "budget.unit",
        &old.budget.unit,
        &new.budget.unit,
    );
    push_change(
        &mut changes,
        "budget.truncated",
        old.budget.truncated,
        new.budget.truncated,
    );
    push_change(
        &mut changes,
        "provenance.compiler",
        &old.provenance.compiler,
        &new.provenance.compiler,
    );
    push_change(
        &mut changes,
        "provenance.compiler_version",
        &old.provenance.compiler_version,
        &new.provenance.compiler_version,
    );
    push_change(
        &mut changes,
        "provenance.surface",
        &old.provenance.surface,
        &new.provenance.surface,
    );
    push_change(
        &mut changes,
        "provenance.source_adapters",
        &old.provenance.source_adapters,
        &new.provenance.source_adapters,
    );
    changes
}

fn push_change<T>(changes: &mut Vec<String>, field: &str, old: T, new: T)
where
    T: PartialEq + std::fmt::Debug,
{
    if old != new {
        changes.push(format!(
            "{field}: {} -> {}",
            clean_debug(&old),
            clean_debug(&new)
        ));
    }
}

fn clean_debug<T: std::fmt::Debug>(value: &T) -> String {
    format!("{value:?}").trim_matches('"').to_string()
}

fn section_map(packet: &ContextPacket) -> Result<BTreeMap<String, Value>> {
    packet
        .sections
        .iter()
        .map(|section| {
            serde_json::to_value(section)
                .map(|value| (section.id.clone(), value))
                .context("failed to serialize packet section for diff")
        })
        .collect()
}

fn item_map(packet: &ContextPacket) -> Result<BTreeMap<String, Value>> {
    let mut items = BTreeMap::new();
    for section in &packet.sections {
        for item in &section.items {
            let value =
                serde_json::to_value(item).context("failed to serialize packet item for diff")?;
            items.insert(item.id.clone(), value);
        }
    }
    Ok(items)
}

fn removed_keys(old: &BTreeMap<String, Value>, new: &BTreeMap<String, Value>) -> Vec<String> {
    old.keys()
        .filter(|key| !new.contains_key(*key))
        .cloned()
        .collect()
}

fn added_keys(old: &BTreeMap<String, Value>, new: &BTreeMap<String, Value>) -> Vec<String> {
    new.keys()
        .filter(|key| !old.contains_key(*key))
        .cloned()
        .collect()
}

fn changed_keys(old: &BTreeMap<String, Value>, new: &BTreeMap<String, Value>) -> Vec<String> {
    old.iter()
        .filter_map(|(key, old_value)| match new.get(key) {
            Some(new_value) if old_value != new_value => Some(key.clone()),
            _ => None,
        })
        .collect()
}

fn diff_warnings(old: &ContextPacket, new: &ContextPacket) -> Vec<String> {
    let old_warnings: BTreeMap<_, _> = old
        .warnings
        .iter()
        .map(|warning| (warning.code.clone(), warning))
        .collect();
    let new_warnings: BTreeMap<_, _> = new
        .warnings
        .iter()
        .map(|warning| (warning.code.clone(), warning))
        .collect();

    let mut changes = Vec::new();
    for code in removed_warning_codes(&old_warnings, &new_warnings) {
        changes.push(format!("removed warning: {code}"));
    }
    for code in added_warning_codes(&old_warnings, &new_warnings) {
        changes.push(format!("added warning: {code}"));
    }
    for (code, old_warning) in &old_warnings {
        if let Some(new_warning) = new_warnings.get(code) {
            if *old_warning != *new_warning {
                changes.push(format!("changed warning: {code}"));
            }
        }
    }
    changes
}

fn removed_warning_codes<T>(old: &BTreeMap<String, T>, new: &BTreeMap<String, T>) -> Vec<String> {
    old.keys()
        .filter(|key| !new.contains_key(*key))
        .cloned()
        .collect()
}

fn added_warning_codes<T>(old: &BTreeMap<String, T>, new: &BTreeMap<String, T>) -> Vec<String> {
    new.keys()
        .filter(|key| !old.contains_key(*key))
        .cloned()
        .collect()
}

fn format_diff_report(report: &PacketDiffReport) -> String {
    format!(
        "ContextPacket diff\n\
         old_id: {}\n\
         new_id: {}\n\
         metadata_changes: {}\n\
         removed_sections: {}\n\
         added_sections: {}\n\
         changed_sections: {}\n\
         removed_items: {}\n\
         added_items: {}\n\
         changed_items: {}\n\
         warning_changes: {}\n\
         summary: {}",
        report.old_id,
        report.new_id,
        format_list(&report.metadata_changes),
        format_list(&report.removed_sections),
        format_list(&report.added_sections),
        format_list(&report.changed_sections),
        format_list(&report.removed_items),
        format_list(&report.added_items),
        format_list(&report.changed_items),
        format_list(&report.warning_changes),
        report.summary
    )
}

fn format_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

fn grade_packet_text(packet_text: &str, task_text: &str, json: bool) -> Result<String> {
    let packet_value: Value =
        serde_json::from_str(packet_text).context("invalid ContextPacket JSON")?;
    let mut pre_deserialization_errors = Vec::new();
    validate_secret_like_values(&packet_value, &mut pre_deserialization_errors);
    if !pre_deserialization_errors.is_empty() {
        bail!(
            "ContextPacket validation failed: {}",
            pre_deserialization_errors.join("; ")
        );
    }

    let packet: ContextPacket = serde_json::from_value(packet_value.clone())
        .map_err(|_| anyhow::anyhow!("JSON did not match the ContextPacket v2 artifact shape"))?;
    let validation = validate_packet_value(&packet_value, &packet, false);
    if !validation.is_valid() {
        bail!(
            "ContextPacket validation failed: {}",
            validation.errors.join("; ")
        );
    }

    let task: TaskSpec = serde_json::from_str(task_text).context("invalid task spec JSON")?;
    if let Err(errors) = task.validate() {
        bail!("task spec validation failed: {}", errors.join("; "));
    }

    let report = PacketQualityReport::grade(&packet, &task);
    if json {
        serde_json::to_string_pretty(&report).context("failed to serialize packet quality report")
    } else {
        Ok(format_quality_report(&report))
    }
}

fn format_quality_report(report: &PacketQualityReport) -> String {
    format!(
        "ContextPacket quality\n\
         recommendation: {:?}\n\
         average_score: {:.2}\n\
         scores.relevance: {}\n\
         scores.completeness: {}\n\
         scores.specificity: {}\n\
         scores.freshness: {}\n\
         scores.grounding: {}\n\
         scores.concision: {}\n\
         scores.noise_absence: {}\n\
         target_coverage_ratio: {:.3}\n\
         validation_coverage_ratio: {:.3}\n\
         warning_penalty: {:.3}\n\
         missed_critical_context: {}\n\
         hallucinated_or_stale_context: {}\n\
         reasons:\n{}",
        report.recommendation,
        report.scores.average(),
        report.scores.relevance,
        report.scores.completeness,
        report.scores.specificity,
        report.scores.freshness,
        report.scores.grounding,
        report.scores.concision,
        report.scores.noise_absence,
        report.target_coverage_ratio,
        report.validation_coverage_ratio,
        report.warning_penalty,
        report.missed_critical_context,
        report.hallucinated_or_stale_context,
        format_reasons(&report.reasons)
    )
}

fn format_reasons(reasons: &[String]) -> String {
    if reasons.is_empty() {
        return "  - none".to_string();
    }
    reasons
        .iter()
        .map(|reason| format!("  - {reason}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn inspect_context_packet(packet: &ContextPacket) -> PacketInspectionReport {
    let item_count = packet
        .sections
        .iter()
        .map(|section| section.items.len())
        .sum();
    let degraded = !packet.warnings.is_empty()
        || packet.low_confidence_fallback
        || packet.retrieval.fallback_reason.is_some()
        || packet.budget.truncated;

    PacketInspectionReport {
        schema_version: packet.schema_version,
        id: packet.id.clone(),
        workspace_id: packet.workspace_id.clone(),
        query: packet.query.clone(),
        created_at: packet.created_at.to_string(),
        git_ref: packet.git_ref.clone(),
        route: packet.route.clone(),
        confidence: packet.confidence.clone(),
        budget: PacketBudgetInspection {
            used_units: packet.budget.used_units,
            max_units: packet.budget.max_units,
            unit: packet.budget.unit.clone(),
            truncated: packet.budget.truncated,
        },
        provenance: PacketProvenanceInspection {
            compiler: packet.provenance.compiler.clone(),
            compiler_version: packet.provenance.compiler_version.clone(),
            surface: packet.provenance.surface.clone(),
            generated_at: packet.provenance.generated_at.to_string(),
            source_adapters: packet.provenance.source_adapters.clone(),
        },
        section_count: packet.sections.len(),
        item_count,
        warning_count: packet.warnings.len(),
        degraded,
        low_confidence_fallback: packet.low_confidence_fallback,
    }
}

fn format_inspection_report(report: &PacketInspectionReport) -> String {
    let git_ref = report.git_ref.as_deref().unwrap_or("none");
    let source_adapters = if report.provenance.source_adapters.is_empty() {
        "none".to_string()
    } else {
        report.provenance.source_adapters.join(", ")
    };

    format!(
        "ContextPacket inspection\n\
         schema_version: {}\n\
         id: {}\n\
         workspace_id: {}\n\
         query: {}\n\
         created_at: {}\n\
         git_ref: {}\n\
         route: {}\n\
         confidence: {}\n\
         budget: {}/{} {} (truncated: {})\n\
         provenance.compiler: {}\n\
         provenance.compiler_version: {}\n\
         provenance.surface: {}\n\
         provenance.generated_at: {}\n\
         provenance.source_adapters: {}\n\
         sections: {}\n\
         items: {}\n\
         warnings: {}\n\
         degraded: {}\n\
         low_confidence_fallback: {}",
        report.schema_version,
        report.id,
        report.workspace_id,
        report.query,
        report.created_at,
        git_ref,
        report.route,
        report.confidence,
        report.budget.used_units,
        report.budget.max_units,
        report.budget.unit,
        report.budget.truncated,
        report.provenance.compiler,
        report.provenance.compiler_version,
        report.provenance.surface,
        report.provenance.generated_at,
        source_adapters,
        report.section_count,
        report.item_count,
        report.warning_count,
        report.degraded,
        report.low_confidence_fallback
    )
}

fn validate_packet_value(
    value: &Value,
    packet: &ContextPacket,
    strict: bool,
) -> PacketValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if packet.schema_version != CONTEXT_PACKET_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {CONTEXT_PACKET_SCHEMA_VERSION}; found {}",
            packet.schema_version
        ));
    }

    require_non_empty(&mut errors, "id", &packet.id);
    require_non_empty(&mut errors, "workspace_id", &packet.workspace_id);
    require_non_empty(&mut errors, "query", &packet.query);
    require_non_empty(&mut errors, "route", &packet.route);
    require_non_empty(&mut errors, "confidence", &packet.confidence);
    require_non_empty(&mut errors, "budget.unit", &packet.budget.unit);

    validate_provenance_presence(value, &mut errors);
    validate_provenance(packet, &mut errors);
    validate_sections(packet, &mut errors);
    validate_warnings(packet, strict, &mut errors, &mut warnings);
    validate_secret_like_values(value, &mut errors);

    PacketValidationReport {
        valid: errors.is_empty(),
        strict,
        errors,
        warnings,
    }
}

fn validate_provenance_presence(value: &Value, errors: &mut Vec<String>) {
    let Some(provenance) = value.get("provenance") else {
        errors.push("provenance must exist for ContextPacket v2 artifacts".to_string());
        return;
    };

    if !provenance.is_object() {
        errors.push("provenance must be an object".to_string());
    }
}

fn validate_provenance(packet: &ContextPacket, errors: &mut Vec<String>) {
    require_non_empty(errors, "provenance.compiler", &packet.provenance.compiler);
    require_non_empty(
        errors,
        "provenance.compiler_version",
        &packet.provenance.compiler_version,
    );
    require_non_empty(errors, "provenance.surface", &packet.provenance.surface);
    require_non_empty(
        errors,
        "provenance.workspace_id",
        &packet.provenance.workspace_id,
    );

    if packet.provenance.surface.trim() == "unknown" {
        errors.push("provenance.surface must identify the producing surface".to_string());
    }

    if packet.provenance.source_adapters.is_empty() {
        errors.push("provenance.source_adapters must include at least one adapter".to_string());
    }

    for (index, adapter) in packet.provenance.source_adapters.iter().enumerate() {
        require_non_empty(
            errors,
            &format!("provenance.source_adapters[{index}]"),
            adapter,
        );
    }
}

fn validate_sections(packet: &ContextPacket, errors: &mut Vec<String>) {
    if packet.sections.is_empty() {
        errors.push("sections must include at least one section".to_string());
        return;
    }

    for (section_index, section) in packet.sections.iter().enumerate() {
        let section_path = format!("sections[{section_index}]");
        require_non_empty(errors, &format!("{section_path}.id"), &section.id);
        require_non_empty(errors, &format!("{section_path}.title"), &section.title);

        if section.items.is_empty() {
            errors.push(format!(
                "{section_path}.items must include at least one item"
            ));
        }

        for (item_index, item) in section.items.iter().enumerate() {
            let item_path = format!("{section_path}.items[{item_index}]");
            require_non_empty(errors, &format!("{item_path}.id"), &item.id);
            require_non_empty(errors, &format!("{item_path}.title"), &item.title);
            require_non_empty(errors, &format!("{item_path}.body"), &item.body);
            require_non_empty(
                errors,
                &format!("{item_path}.source.kind"),
                &item.source.kind,
            );
            require_non_empty(errors, &format!("{item_path}.source.uri"), &item.source.uri);
            require_non_empty(
                errors,
                &format!("{item_path}.selected_reason"),
                &item.selected_reason,
            );
        }
    }
}

fn validate_warnings(
    packet: &ContextPacket,
    strict: bool,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let packet_has_degraded_state = !packet.warnings.is_empty()
        || packet.low_confidence_fallback
        || packet.retrieval.fallback_reason.is_some()
        || packet.budget.truncated;

    if !packet_has_degraded_state {
        return;
    }

    if strict {
        errors.push("strict mode rejects packet warnings or degraded state".to_string());
    } else {
        warnings.push("packet includes warnings or degraded state".to_string());
    }
}

fn validate_secret_like_values(value: &Value, errors: &mut Vec<String>) {
    collect_secret_like_values(value, "$", errors);
}

fn collect_secret_like_values(value: &Value, path: &str, errors: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                collect_secret_like_values(child, &format!("{path}.{key}"), errors);
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_secret_like_values(child, &format!("{path}[{index}]"), errors);
            }
        }
        Value::String(text) => {
            if looks_secret_like(path, text) {
                errors.push(format!(
                    "{path} contains secret-looking content: [REDACTED]"
                ));
            }
        }
        Value::Bool(_) | Value::Number(_) | Value::Null => {}
    }
}

fn looks_secret_like(path: &str, text: &str) -> bool {
    let lower_text = text.to_ascii_lowercase();
    let lower_path = path.to_ascii_lowercase();
    let has_secret_marker = lower_text.contains("api_key=")
        || lower_text.contains("apikey=")
        || lower_text.contains("access_token=")
        || lower_text.contains("password=")
        || lower_text.contains("secret=")
        || lower_text.contains("bearer ")
        || lower_path.contains("api_key")
        || lower_path.contains("apikey")
        || lower_path.contains("access_token")
        || lower_path.contains("password")
        || lower_path.contains("secret");

    has_secret_marker && text.chars().filter(char::is_ascii_alphanumeric).count() >= 12
}

fn require_non_empty(errors: &mut Vec<String>, path: &str, value: &str) {
    if value.trim().is_empty() {
        errors.push(format!("{path} must not be empty"));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::PacketRenderFormat;

    const MINIMAL_PACKET: &str = include_str!("../../docs/examples/context-packet-v2-minimal.json");

    #[test]
    fn validates_documented_minimal_v2_packet() {
        let report = super::validate_packet_text(MINIMAL_PACKET, false)
            .expect("minimal packet should deserialize and validate");

        assert!(report.is_valid());
        assert!(report.errors.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn rejects_invalid_schema_version() {
        let mut packet = minimal_packet_value();
        packet["schema_version"] = Value::from(1);

        let report = validate_value(packet, false);

        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("schema_version must be 2"))
        );
    }

    #[test]
    fn rejects_missing_provenance_field() {
        let mut packet = minimal_packet_value();
        packet
            .as_object_mut()
            .expect("minimal packet fixture is an object")
            .remove("provenance");

        let report = validate_value(packet, false);

        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("provenance"))
        );
    }

    #[test]
    fn strict_mode_rejects_packet_warnings() {
        let mut packet = minimal_packet_value();
        packet["warnings"] = serde_json::json!([{
            "severity": "warning",
            "code": "degraded_memory",
            "message": "used fallback retrieval"
        }]);

        let non_strict_report = validate_value(packet.clone(), false);
        let strict_report = validate_value(packet, true);

        assert!(non_strict_report.is_valid());
        assert!(!non_strict_report.warnings.is_empty());
        assert!(!strict_report.is_valid());
        assert!(
            strict_report
                .errors
                .iter()
                .any(|error| error.contains("strict mode"))
        );
    }

    #[test]
    fn rejects_empty_item_source_and_selected_reason() {
        let mut packet = minimal_packet_value();
        packet["sections"][0]["items"][0]["source"]["kind"] = Value::from(" ");
        packet["sections"][0]["items"][0]["selected_reason"] = Value::from("");

        let report = validate_value(packet, false);

        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("source.kind"))
        );
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("selected_reason"))
        );
    }

    #[test]
    fn rejects_secret_like_text_without_echoing_secret() {
        let mut packet = minimal_packet_value();
        let secret_value = ["access", "_token=", "abc", "123", "secret", "456"].concat();
        let secret_fragment = ["abc", "123", "secret", "456"].concat();
        packet["sections"][0]["items"][0]["body"] = Value::from(secret_value);

        let report = validate_value(packet, false);

        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("[REDACTED]") && error.contains("body"))
        );
        assert!(
            report
                .errors
                .iter()
                .all(|error| !error.contains(&secret_fragment))
        );
    }

    #[test]
    fn rejects_secret_like_text_before_shape_errors_can_echo_it() {
        let mut packet = minimal_packet_value();
        let secret_value = ["bear", "er ", "abc", "123", "secret", "456"].concat();
        let secret_fragment = ["abc", "123", "secret", "456"].concat();
        packet["schema_version"] = Value::from(secret_value);

        let report = super::validate_packet_text(&packet.to_string(), false)
            .expect("secret scan should report before typed deserialization");

        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("[REDACTED]") && error.contains("schema_version"))
        );
        assert!(
            report
                .errors
                .iter()
                .all(|error| !error.contains(&secret_fragment))
        );
    }

    #[test]
    fn inspects_documented_minimal_v2_packet_without_body_text() {
        let output = super::inspect_packet_text(MINIMAL_PACKET, false)
            .expect("minimal packet should inspect successfully");

        assert!(output.contains("ContextPacket inspection"));
        assert!(output.contains("schema_version: 2"));
        assert!(output.contains("id: ctx-example-minimal-v2"));
        assert!(output.contains("workspace_id: layers"));
        assert!(output.contains("query: What should I know before editing README?"));
        assert!(output.contains("created_at: 2026-04-27 00:00:00 UTC"));
        assert!(output.contains("git_ref: none"));
        assert!(output.contains("route: preflight"));
        assert!(output.contains("confidence: high"));
        assert!(output.contains("budget: 6/1200 words (truncated: false)"));
        assert!(output.contains("provenance.compiler: layers-context-packet"));
        assert!(output.contains("provenance.surface: preflight"));
        assert!(output.contains("provenance.source_adapters: workspace"));
        assert!(output.contains("sections: 1"));
        assert!(output.contains("items: 1"));
        assert!(output.contains("warnings: 0"));
        assert!(output.contains("degraded: false"));
        assert!(!output.contains("Branch: main"));
        assert!(!output.contains("Dirty: false"));
    }

    #[test]
    fn inspects_documented_minimal_v2_packet_as_json() {
        let output = super::inspect_packet_text(MINIMAL_PACKET, true)
            .expect("minimal packet should inspect successfully");
        let value: Value = serde_json::from_str(&output).expect("inspection output should be JSON");

        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["id"], "ctx-example-minimal-v2");
        assert_eq!(value["workspace_id"], "layers");
        assert_eq!(value["query"], "What should I know before editing README?");
        assert_eq!(value["git_ref"], Value::Null);
        assert_eq!(value["route"], "preflight");
        assert_eq!(value["confidence"], "high");
        assert_eq!(value["budget"]["used_units"], 6);
        assert_eq!(value["budget"]["max_units"], 1200);
        assert_eq!(value["provenance"]["compiler"], "layers-context-packet");
        assert_eq!(
            value["provenance"]["source_adapters"],
            serde_json::json!(["workspace"])
        );
        assert_eq!(value["section_count"], 1);
        assert_eq!(value["item_count"], 1);
        assert_eq!(value["warning_count"], 0);
        assert_eq!(value["degraded"], false);
        assert_eq!(value["low_confidence_fallback"], false);
        assert!(
            output
                .find("schema_version")
                .expect("schema_version appears")
                < output.find("workspace_id").expect("workspace_id appears")
        );
    }

    #[test]
    fn renders_documented_minimal_v2_packet_as_markdown() {
        let output = super::render_packet_text(MINIMAL_PACKET, PacketRenderFormat::Markdown)
            .expect("minimal packet should render as markdown");

        assert!(output.contains("# Layers Context Packet"));
        assert!(output.contains("Task: What should I know before editing README?"));
        assert!(output.contains("## Workspace State"));
        assert!(output.contains("Branch: main"));
        assert!(output.contains("Source: git status --porcelain=v1 (workspace)"));
    }

    #[test]
    fn renders_documented_minimal_v2_packet_as_agent_prompt() {
        let output = super::render_packet_text(MINIMAL_PACKET, PacketRenderFormat::AgentPrompt)
            .expect("minimal packet should render as an agent prompt");

        assert!(output.starts_with("<layers_context_packet>"));
        assert!(output.contains("<task>What should I know before editing README?</task>"));
        assert!(output.contains("## Workspace State"));
        assert!(output.contains("Branch: main"));
        assert!(output.contains(
            "Selected because: Workspace state changes how agents should interpret context."
        ));
    }

    #[test]
    fn renders_documented_minimal_v2_packet_as_json() {
        let output = super::render_packet_text(MINIMAL_PACKET, PacketRenderFormat::Json)
            .expect("minimal packet should render as JSON");
        let value: Value = serde_json::from_str(&output).expect("render output should be JSON");

        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["id"], "ctx-example-minimal-v2");
        assert_eq!(
            value["sections"][0]["items"][0]["body"],
            "Branch: main\nDirty: false"
        );
    }

    #[test]
    fn renders_documented_minimal_v2_packet_as_objective_brief() {
        let output = super::render_packet_text(MINIMAL_PACKET, PacketRenderFormat::ObjectiveBrief)
            .expect("minimal packet should render as an objective brief");

        assert!(output.contains("# Objective Brief"));
        assert!(output.contains("## Objective"));
        assert!(output.contains("What should I know before editing README?"));
        assert!(output.contains("## Context Constraints"));
        assert!(output.contains("confidence: high"));
        assert!(output.contains("## Cited Context"));
        assert!(output.contains("- Workspace State / Clean workspace"));
        assert!(output.contains("source: git status --porcelain=v1 (workspace)"));
        assert!(output.contains("## Validation Plan"));
        assert!(output.contains("No explicit validation commands were captured in this packet."));
        assert!(output.contains("## Handoff Expectations"));
        assert!(output.contains(
            "If the cited context is insufficient, stop and request an updated ContextPacket"
        ));
    }

    #[test]
    fn diffs_documented_packets_as_text_without_body_echo() {
        let old_packet = minimal_packet_value();
        let mut new_packet = minimal_packet_value();
        new_packet["id"] = Value::from("ctx-example-minimal-v2-new");
        new_packet["sections"][0]["items"][0]["body"] = Value::from("Branch: main\nDirty: true");
        new_packet["sections"][0]["items"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": "context-policy-1",
                "title": "LAYERS.md",
                "body": "Policy: run tests before handoff",
                "source": {
                    "kind": "context_policy",
                    "uri": "LAYERS.md",
                    "repo_path": "LAYERS.md"
                },
                "token_estimate": 5,
                "selected_reason": "repo-owned policy constrains agent work",
                "tags": ["policy", "repo-owned"]
            }));

        let output =
            super::diff_packet_text(&old_packet.to_string(), &new_packet.to_string(), false)
                .expect("valid packets should diff");

        assert!(output.contains("ContextPacket diff"));
        assert!(output.contains("old_id: ctx-example-minimal-v2"));
        assert!(output.contains("new_id: ctx-example-minimal-v2-new"));
        assert!(output.contains("added_items: context-policy-1"));
        assert!(output.contains("changed_items: workspace-state"));
        assert!(!output.contains("Dirty: true"));
        assert!(!output.contains("Policy: run tests"));
    }

    #[test]
    fn diffs_documented_packets_as_json() {
        let old_packet = minimal_packet_value();
        let mut new_packet = minimal_packet_value();
        new_packet["id"] = Value::from("ctx-example-minimal-v2-new");
        new_packet["confidence"] = Value::from("medium");
        new_packet["sections"][0]["items"]
            .as_array_mut()
            .unwrap()
            .clear();
        new_packet["sections"][0]["items"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": "context-policy-1",
                "title": "LAYERS.md",
                "body": "Policy: run tests before handoff",
                "source": {
                    "kind": "context_policy",
                    "uri": "LAYERS.md",
                    "repo_path": "LAYERS.md"
                },
                "token_estimate": 5,
                "selected_reason": "repo-owned policy constrains agent work",
                "tags": ["policy", "repo-owned"]
            }));

        let output =
            super::diff_packet_text(&old_packet.to_string(), &new_packet.to_string(), true)
                .expect("valid packets should diff as JSON");
        let value: Value = serde_json::from_str(&output).expect("diff output should be JSON");

        assert_eq!(value["old_id"], "ctx-example-minimal-v2");
        assert_eq!(value["new_id"], "ctx-example-minimal-v2-new");
        assert_eq!(value["metadata_changes"][0], "confidence: high -> medium");
        assert_eq!(
            value["removed_items"],
            serde_json::json!(["workspace-state"])
        );
        assert_eq!(
            value["added_items"],
            serde_json::json!(["context-policy-1"])
        );
        assert_eq!(value["summary"], "1 removed, 1 added, 0 changed");
    }

    #[test]
    fn diff_rejects_invalid_packets_without_echoing_secret_like_text() {
        let old_packet = minimal_packet_value();
        let mut new_packet = minimal_packet_value();
        let secret_value = ["api_key=", "abc", "123", "secret", "456"].concat();
        let secret_fragment = ["abc", "123", "secret", "456"].concat();
        new_packet["sections"][0]["items"][0]["body"] = Value::from(secret_value);

        let error =
            super::diff_packet_text(&old_packet.to_string(), &new_packet.to_string(), false)
                .expect_err("secret-looking packet should fail diff")
                .to_string();

        assert!(error.contains("ContextPacket validation failed"));
        assert!(error.contains("[REDACTED]") || !error.contains(&secret_fragment));
        assert!(!error.contains(&secret_fragment));
    }

    #[test]
    fn grades_documented_minimal_v2_packet_as_json() {
        let output = super::grade_packet_text(MINIMAL_PACKET, orientation_task_json(), true)
            .expect("minimal packet should grade successfully");
        let value: Value = serde_json::from_str(&output).expect("grade output should be JSON");

        assert_eq!(value["recommendation"], "inject_full");
        assert_eq!(value["missed_critical_context"], false);
        assert_eq!(value["target_coverage_ratio"], 1.0);
        assert_eq!(value["scores"]["relevance"], 5);
    }

    #[test]
    fn grades_documented_minimal_v2_packet_as_text() {
        let output = super::grade_packet_text(MINIMAL_PACKET, orientation_task_json(), false)
            .expect("minimal packet should grade successfully");

        assert!(output.contains("ContextPacket quality"));
        assert!(output.contains("recommendation: InjectFull"));
        assert!(output.contains("target_coverage_ratio: 1.000"));
        assert!(output.contains("reasons:"));
    }

    #[test]
    fn grade_rejects_invalid_task_spec() {
        let invalid_task = r#"{
          "task_id":"bad",
          "title":"Missing targets",
          "prompt":"Fix the bug",
          "category":"bugfix"
        }"#;

        let error = super::grade_packet_text(MINIMAL_PACKET, invalid_task, false)
            .expect_err("code-heavy task without targets should fail");

        assert!(error.to_string().contains("task spec validation failed"));
    }

    #[test]
    fn inspect_rejects_invalid_packets_without_echoing_secret_like_text() {
        let mut packet = minimal_packet_value();
        let secret_value = ["pass", "word=", "abc", "123", "secret", "456"].concat();
        let secret_fragment = ["abc", "123", "secret", "456"].concat();
        packet["sections"][0]["items"][0]["body"] = Value::from(secret_value);

        let error = super::inspect_packet_text(&packet.to_string(), false)
            .expect_err("secret-looking packet should fail inspection")
            .to_string();

        assert!(error.contains("ContextPacket validation failed"));
        assert!(error.contains("[REDACTED]") || !error.contains(&secret_fragment));
        assert!(!error.contains(&secret_fragment));
    }

    fn minimal_packet_value() -> Value {
        serde_json::from_str(MINIMAL_PACKET).expect("minimal packet fixture must be valid JSON")
    }

    fn orientation_task_json() -> &'static str {
        r#"{
          "task_id":"orientation-readme-1",
          "title":"Inspect README context",
          "prompt":"What should I know before editing README?",
          "category":"orientation",
          "expected_validation_commands":["git status --porcelain=v1"]
        }"#
    }

    fn validate_value(value: Value, strict: bool) -> super::PacketValidationReport {
        let text = serde_json::to_string(&value).expect("test packet value should serialize");
        super::validate_packet_text(&text, strict)
            .expect("test packet should deserialize as ContextPacket")
    }
}
