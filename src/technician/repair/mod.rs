//! Repair engine for the technician.
//!
//! Each repairable `DiagnosisKind` has a concrete handler that mutates the
//! filesystem (truncate corrupt JSONL, create UC config stub, reset circuit-
//! breaker state) and then returns an outcome. A post-repair verification
//! re-runs the relevant detector to confirm the fix took hold.

use std::fs;
use std::io::Write;
use std::path::Path;

use super::data::{
    Diagnosis, DiagnosisKind, RepairBudget, RepairOutcome, RepairRecord, TECHNICIAN_SCHEMA_VERSION,
};

/// Returns true if the given diagnosis has an autonomous repair available.
pub fn can_repair(kind: &DiagnosisKind) -> bool {
    kind.autonomously_repairable()
}

/// Attempt to repair a diagnosis within the given budget.
///
/// When `apply` is false, returns a `SkippedDryRun` record describing the
/// repair that *would* be taken. When `apply` is true, dispatches to the
/// concrete handler which executes the repair and decrements the budget.
/// Returns `None` if the diagnosis kind has no autonomous repair available.
pub fn attempt_repair(
    diagnosis: &Diagnosis,
    budget: &mut RepairBudget,
    cycle_id: &str,
    apply: bool,
) -> Option<RepairRecord> {
    if !can_repair(&diagnosis.kind) {
        return None;
    }

    if !apply {
        return Some(dry_run_record(diagnosis, cycle_id));
    }

    // Dispatch to concrete repair handler
    match &diagnosis.kind {
        DiagnosisKind::CouncilTracesJsonlCorrupt
        | DiagnosisKind::CouncilAuditJsonlCorrupt
        | DiagnosisKind::RouteCorrectionsJsonlCorrupt => {
            repair_truncate_jsonl(diagnosis, budget, cycle_id)
        }
        DiagnosisKind::UcConfigMissing => repair_uc_config_stub(diagnosis, budget, cycle_id),
        DiagnosisKind::CircuitBreakerTripped => {
            repair_circuit_breaker_reset(diagnosis, budget, cycle_id)
        }
        DiagnosisKind::SentryNewError => repair_sentry_acknowledge(diagnosis, budget, cycle_id),
        _ => None,
    }
}

/// Build a dry-run record that shows what repair *would* be applied.
fn dry_run_record(diagnosis: &Diagnosis, cycle_id: &str) -> RepairRecord {
    let repair_action = match &diagnosis.kind {
        DiagnosisKind::CouncilTracesJsonlCorrupt
        | DiagnosisKind::CouncilAuditJsonlCorrupt
        | DiagnosisKind::RouteCorrectionsJsonlCorrupt => "jsonl_truncate",
        DiagnosisKind::UcConfigMissing => "uc_config_stub",
        DiagnosisKind::CircuitBreakerTripped => "cb_reset",
        DiagnosisKind::SentryNewError => "sentry_acknowledge",
        _ => "unknown",
    };

    let path = match &diagnosis.kind {
        DiagnosisKind::CouncilTracesJsonlCorrupt
        | DiagnosisKind::CouncilAuditJsonlCorrupt
        | DiagnosisKind::RouteCorrectionsJsonlCorrupt => resolve_jsonl_path(diagnosis),
        DiagnosisKind::UcConfigMissing => {
            Some(crate::config::uc_config_path().display().to_string())
        }
        DiagnosisKind::CircuitBreakerTripped => diagnosis
            .context
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(|id| {
                crate::config::memoryport_dir()
                    .join("council-runs")
                    .join(id)
                    .join("run.json")
                    .display()
                    .to_string()
            }),
        _ => None,
    };

    RepairRecord {
        schema_version: TECHNICIAN_SCHEMA_VERSION,
        ts: crate::util::iso_now(),
        cycle_id: cycle_id.to_string(),
        diagnosis: diagnosis.kind.name().to_string(),
        repair_action: repair_action.to_string(),
        path,
        lines_removed: None,
        outcome: RepairOutcome::SkippedDryRun,
    }
}

/// Verify that a repair resolved the fault by re-running the relevant
/// detection logic. Returns `(verified, note)`.
pub fn verify_repair(diagnosis: &Diagnosis) -> (bool, String) {
    match &diagnosis.kind {
        DiagnosisKind::CouncilTracesJsonlCorrupt | DiagnosisKind::CouncilAuditJsonlCorrupt => {
            let recurrence = super::detection::detect_council_artifacts();
            let still_corrupt = recurrence.iter().any(|d| {
                matches!(
                    d.kind,
                    DiagnosisKind::CouncilTracesJsonlCorrupt
                        | DiagnosisKind::CouncilAuditJsonlCorrupt
                )
            });
            if still_corrupt {
                (
                    false,
                    "JSONL still contains corrupt lines after truncation".into(),
                )
            } else {
                (true, "JSONL validates clean after truncation".into())
            }
        }
        DiagnosisKind::RouteCorrectionsJsonlCorrupt => {
            let recurrence = super::detection::detect_route_corrections();
            if recurrence.is_empty() {
                (true, "route-corrections.jsonl validates clean".into())
            } else {
                (false, "route-corrections.jsonl still corrupt".into())
            }
        }
        DiagnosisKind::UcConfigMissing => {
            let path = crate::config::uc_config_path();
            if path.exists() {
                (
                    true,
                    format!("uc config stub created at {}", path.display()),
                )
            } else {
                (
                    false,
                    "uc config file still missing after stub creation".into(),
                )
            }
        }
        DiagnosisKind::CircuitBreakerTripped => {
            // CB reset is verified by checking the run.json status was updated
            if let Some(run_id) = diagnosis.context.get("run_id").and_then(|v| v.as_str()) {
                let run_dir = crate::config::memoryport_dir()
                    .join("council-runs")
                    .join(run_id);
                let run_json = run_dir.join("run.json");
                if let Ok(content) = fs::read_to_string(&run_json) {
                    if content.contains("\"status\":\"reset\"")
                        || content.contains("\"status\": \"reset\"")
                    {
                        return (true, format!("run {run_id} status set to reset"));
                    }
                }
                (false, format!("run {run_id} not updated to reset status"))
            } else {
                (false, "no run_id in diagnosis context".into())
            }
        }
        DiagnosisKind::SentryNewError => {
            // Sentry acknowledgement is fire-and-forget; we trust the API call
            (true, "sentry issue acknowledged (API call sent)".into())
        }
        _ => (false, "no verification logic for this diagnosis".into()),
    }
}

// ---------------------------------------------------------------------------
// JSONL truncate repair
// ---------------------------------------------------------------------------

/// Truncate a corrupt JSONL file at the last valid line boundary.
///
/// Reads the file, keeps only lines that parse as valid JSON, and rewrites.
/// Returns the number of lines removed.
fn truncate_jsonl_at_valid_boundary(path: &Path) -> Result<usize, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read failed: {e}"))?;
    let mut valid_lines = Vec::new();
    let mut removed = 0usize;

    for line in content.lines() {
        if line.trim().is_empty() {
            valid_lines.push(line.to_string());
            continue;
        }
        if serde_json::from_str::<serde_json::Value>(line).is_ok() {
            valid_lines.push(line.to_string());
        } else {
            removed += 1;
        }
    }

    if removed == 0 {
        return Ok(0);
    }

    // Atomic-ish rewrite: write to .tmp then rename
    let tmp = path.with_extension("jsonl.tmp");
    let mut file = fs::File::create(&tmp).map_err(|e| format!("create tmp: {e}"))?;
    for line in &valid_lines {
        writeln!(file, "{line}").map_err(|e| format!("write: {e}"))?;
    }
    file.flush().map_err(|e| format!("flush: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;

    Ok(removed)
}

/// Resolve the filesystem path for a corrupt-JSONL diagnosis.
fn resolve_jsonl_path(diagnosis: &Diagnosis) -> Option<String> {
    // The detection code puts the file path in context.file or context.path
    diagnosis
        .context
        .get("file")
        .or_else(|| diagnosis.context.get("path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn repair_truncate_jsonl(
    diagnosis: &Diagnosis,
    budget: &mut RepairBudget,
    cycle_id: &str,
) -> Option<RepairRecord> {
    if budget.jsonl_truncate == 0 {
        return Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "jsonl_truncate".to_string(),
            path: resolve_jsonl_path(diagnosis),
            lines_removed: None,
            outcome: RepairOutcome::SkippedNoBudget,
        });
    }

    let Some(path_str) = resolve_jsonl_path(diagnosis) else {
        return Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "jsonl_truncate".to_string(),
            path: None,
            lines_removed: None,
            outcome: RepairOutcome::Failed,
        });
    };

    let path = Path::new(&path_str);
    match truncate_jsonl_at_valid_boundary(path) {
        Ok(removed) => {
            budget.jsonl_truncate -= 1;
            Some(RepairRecord {
                schema_version: TECHNICIAN_SCHEMA_VERSION,
                ts: crate::util::iso_now(),
                cycle_id: cycle_id.to_string(),
                diagnosis: diagnosis.kind.name().to_string(),
                repair_action: "jsonl_truncate".to_string(),
                path: Some(path_str),
                lines_removed: Some(removed),
                outcome: RepairOutcome::Applied,
            })
        }
        Err(_) => Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "jsonl_truncate".to_string(),
            path: Some(path_str),
            lines_removed: None,
            outcome: RepairOutcome::Failed,
        }),
    }
}

// ---------------------------------------------------------------------------
// UC config stub repair
// ---------------------------------------------------------------------------

/// Create a minimal UC config stub so that `uc` can start.
fn repair_uc_config_stub(
    diagnosis: &Diagnosis,
    budget: &mut RepairBudget,
    cycle_id: &str,
) -> Option<RepairRecord> {
    if budget.uc_stub == 0 {
        return Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "uc_config_stub".to_string(),
            path: None,
            lines_removed: None,
            outcome: RepairOutcome::SkippedNoBudget,
        });
    }

    let config_path = crate::config::uc_config_path();
    if let Some(parent) = config_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let stub_content = "# UC config stub created by layers technician
# Edit this file to configure your UC retrieval settings.

[retrieval]
timeout_ms = 500
min_results = 0
";

    match fs::write(&config_path, stub_content) {
        Ok(()) => {
            budget.uc_stub -= 1;
            Some(RepairRecord {
                schema_version: TECHNICIAN_SCHEMA_VERSION,
                ts: crate::util::iso_now(),
                cycle_id: cycle_id.to_string(),
                diagnosis: diagnosis.kind.name().to_string(),
                repair_action: "uc_config_stub".to_string(),
                path: Some(config_path.display().to_string()),
                lines_removed: None,
                outcome: RepairOutcome::Applied,
            })
        }
        Err(_) => Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "uc_config_stub".to_string(),
            path: Some(config_path.display().to_string()),
            lines_removed: None,
            outcome: RepairOutcome::Failed,
        }),
    }
}

// ---------------------------------------------------------------------------
// Circuit-breaker reset repair
// ---------------------------------------------------------------------------

/// Reset a tripped circuit breaker by updating the council run's status
/// from "failed" to "reset", allowing a future council run to retry.
fn repair_circuit_breaker_reset(
    diagnosis: &Diagnosis,
    budget: &mut RepairBudget,
    cycle_id: &str,
) -> Option<RepairRecord> {
    let run_id = diagnosis
        .context
        .get("run_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let budget_count = budget.cb_reset.entry(run_id.to_string()).or_insert(1);
    if *budget_count == 0 {
        return Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "cb_reset".to_string(),
            path: None,
            lines_removed: None,
            outcome: RepairOutcome::SkippedNoBudget,
        });
    }

    let run_dir = crate::config::memoryport_dir()
        .join("council-runs")
        .join(run_id);
    let run_json = run_dir.join("run.json");

    if !run_json.exists() {
        return Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "cb_reset".to_string(),
            path: Some(run_json.display().to_string()),
            lines_removed: None,
            outcome: RepairOutcome::Failed,
        });
    }

    // Read, patch status → "reset", write back
    let Ok(content) = fs::read_to_string(&run_json) else {
        return Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "cb_reset".to_string(),
            path: Some(run_json.display().to_string()),
            lines_removed: None,
            outcome: RepairOutcome::Failed,
        });
    };

    let mut value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => {
            return Some(RepairRecord {
                schema_version: TECHNICIAN_SCHEMA_VERSION,
                ts: crate::util::iso_now(),
                cycle_id: cycle_id.to_string(),
                diagnosis: diagnosis.kind.name().to_string(),
                repair_action: "cb_reset".to_string(),
                path: Some(run_json.display().to_string()),
                lines_removed: None,
                outcome: RepairOutcome::Failed,
            });
        }
    };

    value["status"] = serde_json::json!("reset");
    value["status_reason"] = serde_json::json!(format!("technician cb_reset in cycle {cycle_id}"));

    match fs::write(
        &run_json,
        serde_json::to_string_pretty(&value).unwrap_or_default(),
    ) {
        Ok(()) => {
            *budget_count -= 1;
            Some(RepairRecord {
                schema_version: TECHNICIAN_SCHEMA_VERSION,
                ts: crate::util::iso_now(),
                cycle_id: cycle_id.to_string(),
                diagnosis: diagnosis.kind.name().to_string(),
                repair_action: "cb_reset".to_string(),
                path: Some(run_json.display().to_string()),
                lines_removed: None,
                outcome: RepairOutcome::Applied,
            })
        }
        Err(_) => Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "cb_reset".to_string(),
            path: Some(run_json.display().to_string()),
            lines_removed: None,
            outcome: RepairOutcome::Failed,
        }),
    }
}

// ---------------------------------------------------------------------------
// Sentry acknowledge repair
// ---------------------------------------------------------------------------

/// Acknowledge a Sentry error by resolving it via the API.
fn repair_sentry_acknowledge(
    diagnosis: &Diagnosis,
    _budget: &mut RepairBudget,
    cycle_id: &str,
) -> Option<RepairRecord> {
    let issue_id = diagnosis
        .context
        .get("issue_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if issue_id.is_empty() {
        return Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "sentry_acknowledge".to_string(),
            path: None,
            lines_removed: None,
            outcome: RepairOutcome::Failed,
        });
    }

    let config = crate::plugins::sentry::SentryConfig::default();
    let client = crate::plugins::sentry::SentryClient::new(config);

    match client.resolve_issue(issue_id) {
        Ok(()) => {
            let _ = client.add_issue_comment(
                issue_id,
                &format!("[automated] Technician acknowledged in cycle {cycle_id}"),
            );
            Some(RepairRecord {
                schema_version: TECHNICIAN_SCHEMA_VERSION,
                ts: crate::util::iso_now(),
                cycle_id: cycle_id.to_string(),
                diagnosis: diagnosis.kind.name().to_string(),
                repair_action: "sentry_acknowledge".to_string(),
                path: None,
                lines_removed: None,
                outcome: RepairOutcome::Applied,
            })
        }
        Err(_) => Some(RepairRecord {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: "sentry_acknowledge".to_string(),
            path: None,
            lines_removed: None,
            outcome: RepairOutcome::Failed,
        }),
    }
}
