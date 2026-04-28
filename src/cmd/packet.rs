use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail};
use clap::{Subcommand, ValueEnum};
use layers_core::context_packet::{CONTEXT_PACKET_SCHEMA_VERSION, ContextPacket};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PacketRenderFormat {
    Markdown,
    AgentPrompt,
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
}

pub(crate) fn handle_packet(command: &PacketCommands) -> Result<()> {
    match command {
        PacketCommands::Validate { path, strict, json } => validate_packet(path, *strict, *json),
        PacketCommands::Inspect { path, json } => inspect_packet(path, *json),
        PacketCommands::Render { .. } => bail!("packet render is not implemented yet"),
        PacketCommands::Diff { .. } => bail!("packet diff is not implemented yet"),
    }
}

fn inspect_packet(path: &Path, json: bool) -> Result<()> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read ContextPacket artifact {}", path.display()))?;
    let output = inspect_packet_text(&text, json)?;
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

    fn validate_value(value: Value, strict: bool) -> super::PacketValidationReport {
        let text = serde_json::to_string(&value).expect("test packet value should serialize");
        super::validate_packet_text(&text, strict)
            .expect("test packet should deserialize as ContextPacket")
    }
}
