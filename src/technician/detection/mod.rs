//! Detection functions — one per signal family.
//!
//! Each `detect_*` function runs its checks and returns a `Vec<Diagnosis>`.

use std::fs;
use std::path::Path;

use crate::config::{council_files, memoryport_dir, workspace_root};
use crate::types::CouncilRunRecord;
use crate::uc;
use chrono::{Duration, Utc};

use super::data::{
    Diagnosis, DiagnosisKind, Signal,
};
use super::data::repair::RepairBudget;

// ---------------------------------------------------------------------------
// Plugin registration
// ---------------------------------------------------------------------------

/// Verify that internal plugins (rlef, telemetry) can be instantiated and called.
pub fn detect_plugins() -> Vec<Diagnosis> {
    let mut diagnoses = Vec::new();

    // RLEF plugin: try instantiating and calling select
    match std::panic::catch_unwind(|| {
        let plugin = crate::plugins::rlef::RlefRouterPlugin::new();
        plugin.select(["a", "b", "c"])
    }) {
        Ok(_) => {}
        Err(_) => {
            diagnoses.push(Diagnosis::new(
                DiagnosisKind::PluginPanic,
                "rlef plugin panicked on select()".to_string(),
                serde_json::json!({ "plugin": "rlef" }),
            ));
        }
    }

    // Telemetry plugin: try instantiating and recording a routing decision
    let tempdir = tempfile::TempDir::new().ok();
    if let Some(tmp) = &tempdir {
        let path = tmp.path().join("events.jsonl");
        match crate::plugins::telemetry::TelemetryPlugin::new(tmp.path().to_path_buf())
            .record_routing_decision(
                "test-task",
                crate::router::Route::Both,
                crate::router::Confidence::High,
                None,
            ) {
            Ok(_) => {}
            Err(_) => {
                diagnoses.push(Diagnosis::new(
                    DiagnosisKind::PluginPanic,
                    "telemetry plugin returned error on record_routing_decision".to_string(),
                    serde_json::json!({ "plugin": "telemetry" }),
                ));
            }
        }
    }

    diagnoses
}

// ---------------------------------------------------------------------------
// UC availability
// ---------------------------------------------------------------------------

/// Check that the UC binary is on PATH and the config file exists.
pub fn detect_uc() -> Vec<Diagnosis> {
    let mut diagnoses = Vec::new();

    let available = uc::is_available();

    if !available {
        // Try to distinguish binary-missing from config-missing
        if !which::which("uc").is_ok() {
            diagnoses.push(Diagnosis::new(
                DiagnosisKind::UcBinaryMissing,
                "uc binary not found on PATH".to_string(),
                serde_json::json!({}),
            ));
        } else if !crate::config::uc_config_path().exists() {
            diagnoses.push(Diagnosis::new(
                DiagnosisKind::UcConfigMissing,
                "uc config file missing".to_string(),
                serde_json::json!({
                    "config_path": crate::config::uc_config_path().display().to_string()
                }),
            ));
        } else {
            // is_available returned false for an unknown reason
            diagnoses.push(Diagnosis::new(
                DiagnosisKind::UcBinaryMissing,
                "uc is_available() returned false (unknown reason)".to_string(),
                serde_json::json!({}),
            ));
        }
        return diagnoses;
    }

    // UC is available — do a quick functional check: spawn and verify exit 0
    let retriever = uc::UcRetriever::new(uc::UcOptions {
        timeout_ms: 500,
        min_results: 0,
    });
    let result = retriever.retrieve("technician-health-check", 1);
    if let Some(reason) = &result.fallback_reason {
        if reason.contains("timed out") {
            diagnoses.push(Diagnosis::new(
                DiagnosisKind::UcTimeout,
                "uc retrieve timed out".to_string(),
                serde_json::json!({ "fallback_reason": reason }),
            ));
        } else {
            diagnoses.push(Diagnosis::new(
                DiagnosisKind::UcNonZeroExit,
                format!("uc retrieve failed: {reason}"),
                serde_json::json!({ "fallback_reason": reason }),
            ));
        }
    }

    diagnoses
}

// ---------------------------------------------------------------------------
// GitNexus availability
// ---------------------------------------------------------------------------

/// Check that gitnexus binary is on PATH and the index is not stale.
pub fn detect_gitnexus() -> Vec<Diagnosis> {
    let mut diagnoses = Vec::new();

    if which::which("gitnexus").is_err() {
        diagnoses.push(Diagnosis::new(
            DiagnosisKind::GitNexusBinaryMissing,
            "gitnexus binary not found on PATH".to_string(),
            serde_json::json!({}),
        ));
        return diagnoses;
    }

    // Check index freshness: ~/.layers/ or workspace root
    let index_path = workspace_root().join(".gitnexus");
    if index_path.exists() {
        let meta_path = index_path.join("meta.json");
        if let Ok(metadata) = fs::read_to_string(&meta_path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&metadata) {
                if let Some(ts) = value.get("indexed_at").and_then(|v| v.as_str()) {
                    if let Ok(indexed) = chrono::DateTime::parse_from_rfc3339(ts) {
                        let age = Utc::now() - indexed.with_timezone(&Utc);
                        if age > Duration::hours(48) {
                            diagnoses.push(Diagnosis::new(
                                DiagnosisKind::GitNexusIndexStale,
                                format!("gitnexus index is {} hours old", age.num_hours()),
                                serde_json::json!({
                                    "indexed_at": ts,
                                    "age_hours": age.num_hours()
                                }),
                            ));
                        }
                    }
                }
            }
        }
    }

    diagnoses
}

// ---------------------------------------------------------------------------
// Council artifact integrity
// ---------------------------------------------------------------------------

/// Check all council run directories for missing/corrupt artifacts.
pub fn detect_council_artifacts() -> Vec<Diagnosis> {
    let mut diagnoses = Vec::new();
    let runs_dir = memoryport_dir().join("council-runs");

    if !runs_dir.exists() {
        return diagnoses;
    }

    let entries = match fs::read_dir(&runs_dir) {
        Ok(e) => e,
        Err(_) => return diagnoses,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip archived dirs
        if name.starts_with("archived") {
            continue;
        }

        // Check run.json exists and is valid JSON
        let run_json_path = path.join("run.json");
        if !run_json_path.exists() {
            // Only flag if this run is old enough to be considered settled
            if let Ok(meta) = fs::metadata(&path) {
                if let Ok(age) = meta.modified() {
                    let age_dur = std::time::SystemTime::now()
                        .duration_since(age)
                        .unwrap_or_default();
                    if age_dur.as_secs() > 3600 {
                        // older than 1 hour
                        diagnoses.push(Diagnosis::new(
                            DiagnosisKind::CouncilRunArtifactsMissing,
                            format!("run.json missing for council run {name}"),
                            serde_json::json!({
                                "run_id": name,
                                "artifacts_dir": path.display().to_string()
                            }),
                        ));
                    }
                }
            }
            continue;
        }

        let run_json_content = match fs::read_to_string(&run_json_path) {
            Ok(c) => c,
            Err(_) => {
                diagnoses.push(Diagnosis::new(
                    DiagnosisKind::CouncilRunJsonCorrupt,
                    format!("run.json unreadable for council run {name}"),
                    serde_json::json!({ "run_id": name }),
                ));
                continue;
            }
        };

        let record: CouncilRunRecord = match serde_json::from_str(&run_json_content) {
            Ok(r) => r,
            Err(_) => {
                diagnoses.push(Diagnosis::new(
                    DiagnosisKind::CouncilRunJsonCorrupt,
                    format!("run.json is not valid JSON for council run {name}"),
                    serde_json::json!({ "run_id": name }),
                ));
                continue;
            }
        };

        // Check referenced files exist
        for stage in &record.stages {
            if !stage.stdout_path.is_empty()
                && !Path::new(&stage.stdout_path).exists()
            {
                diagnoses.push(Diagnosis::new(
                    DiagnosisKind::CouncilRunArtifactsMissing,
                    format!(
                        "stage {} stdout file missing for run {}",
                        stage.stage, name
                    ),
                    serde_json::json!({
                        "run_id": name,
                        "stage": stage.stage,
                        "stdout_path": stage.stdout_path
                    }),
                ));
            }
            if !stage.stderr_path.is_empty()
                && !Path::new(&stage.stderr_path).exists()
            {
                diagnoses.push(Diagnosis::new(
                    DiagnosisKind::CouncilRunArtifactsMissing,
                    format!(
                        "stage {} stderr file missing for run {}",
                        stage.stage, name
                    ),
                    serde_json::json!({
                        "run_id": name,
                        "stage": stage.stage,
                        "stderr_path": stage.stderr_path
                    }),
                ));
            }
        }
    }

    // Check JSONL artifact files for corruption
    for (kind, path) in council_files() {
        if !path.exists() {
            continue;
        }
        if let Err(e) = validate_jsonl(&path, 50) {
            let kind = match kind {
                "plan" => "council-plans",
                "trace" => "council-traces",
                "learning" => "council-learnings",
                _ => kind,
            };
            diagnoses.push(Diagnosis::new(
                DiagnosisKind::CouncilTracesJsonlCorrupt,
                format!("{kind}.jsonl has corrupt lines: {e}"),
                serde_json::json!({
                    "file": path.display().to_string(),
                    "kind": kind
                }),
            ));
        }
    }

    diagnoses
}

/// Check a JSONL file for corruption — returns Err with message if bad lines found.
fn validate_jsonl(path: &Path, max_lines: usize) -> Result<(), String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut bad_lines = Vec::new();
    for (i, line) in content.lines().take(max_lines).enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if serde_json::from_str::<serde_json::Value>(line).is_err() {
            bad_lines.push(i + 1);
        }
    }
    if bad_lines.is_empty() {
        Ok(())
    } else {
        Err(format!("bad lines: {bad_lines:?}"))
    }
}

// ---------------------------------------------------------------------------
// Circuit breaker / council failure
// ---------------------------------------------------------------------------

/// Detect council runs that failed due to circuit breaker exhaustion or stage failures.
pub fn detect_circuit_breaker() -> Vec<Diagnosis> {
    let mut diagnoses = Vec::new();
    let runs_dir = memoryport_dir().join("council-runs");
    let cutoff = Utc::now() - Duration::days(7);

    if !runs_dir.exists() {
        return diagnoses;
    }

    let entries = match fs::read_dir(&runs_dir) {
        Ok(e) => e,
        Err(_) => return diagnoses,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with("archived") {
            continue;
        }

        let run_json_path = path.join("run.json");
        let content = match fs::read_to_string(&run_json_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let record: CouncilRunRecord = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Only flag failed/incomplete runs from the last 7 days
        if let Ok(created) = chrono::DateTime::parse_from_rfc3339(&record.created_at) {
            if created.with_timezone(&Utc) < cutoff {
                continue;
            }
        }

        match record.status.as_str() {
            "failed" => {
                let reason = &record.status_reason;
                let diagnosis = if reason.contains("circuit breaker tripped") {
                    DiagnosisKind::CircuitBreakerTripped
                } else if reason.contains("retries_exhausted") {
                    DiagnosisKind::StageRetriesExhausted
                } else if reason.contains("timed_out") || reason.contains("timeout") {
                    DiagnosisKind::StageTimedOut
                } else {
                    DiagnosisKind::CircuitBreakerTripped // general failure
                };

                diagnoses.push(Diagnosis::new(
                    diagnosis,
                    format!("council run {name} failed: {reason}"),
                    serde_json::json!({
                        "run_id": name,
                        "status": record.status,
                        "status_reason": record.status_reason,
                        "artifacts_dir": record.artifacts_dir
                    }),
                ));
            }
            "incomplete" => {
                diagnoses.push(Diagnosis::new(
                    DiagnosisKind::ConvergenceNotReached,
                    format!("council run {name} did not converge"),
                    serde_json::json!({
                        "run_id": name,
                        "status": record.status,
                        "status_reason": record.status_reason
                    }),
                ));
            }
            _ => {}
        }
    }

    diagnoses
}

// ---------------------------------------------------------------------------
// Telemetry health
// ---------------------------------------------------------------------------

/// Check telemetry plugin event file for error rate and latency anomalies.
pub fn detect_telemetry_health() -> Vec<Diagnosis> {
    let mut diagnoses = Vec::new();
    let events_path = crate::plugins::telemetry::events_path();

    if !events_path.exists() {
        diagnoses.push(Diagnosis::new(
            DiagnosisKind::TelemetryFileMissing,
            "telemetry events.jsonl does not exist".to_string(),
            serde_json::json!({ "path": events_path.display().to_string() }),
        ));
        return diagnoses;
    }

    let events = match crate::plugins::telemetry::load_events_from_file(&events_path) {
        Ok(e) => e,
        Err(_) => {
            diagnoses.push(Diagnosis::new(
                DiagnosisKind::TelemetryFileMissing,
                "telemetry events.jsonl could not be parsed".to_string(),
                serde_json::json!({ "path": events_path.display().to_string() }),
            ));
            return diagnoses;
        }
    };

    let total = events.len();
    if total == 0 {
        return diagnoses;
    }

    // Compute error rate from last 100 events
    let recent: Vec<_> = events.iter().rev().take(100).collect();
    let errors = recent
        .iter()
        .filter(|e| {
            matches!(
                e,
                crate::plugins::telemetry::schema::RoutingDecisionEvent { outcome: crate::plugins::telemetry::schema::RoutingOutcome::Failure, .. }
            )
        })
        .count();
    let error_rate = errors as f64 / recent.len() as f64;

    if error_rate > 0.20 {
        diagnoses.push(Diagnosis::new(
            DiagnosisKind::TelemetryErrorRateHigh,
            format!(
                "telemetry error rate {:.1}% ({} errors in last {} events)",
                error_rate * 100.0,
                errors, recent.len()
            ),
            serde_json::json!({
                "error_rate": error_rate,
                "error_count": errors,
                "sample_size": recent.len()
            }),
        ));
    }

    diagnoses
}

// ---------------------------------------------------------------------------
// Route corrections integrity
// ---------------------------------------------------------------------------

/// Check the route-corrections JSONL for corruption.
pub fn detect_route_corrections() -> Vec<Diagnosis> {
    let mut diagnoses = Vec::new();
    let path = crate::router::corrections_path();

    if !path.exists() {
        // Missing is fine — the file is optional
        return diagnoses;
    }

    if let Err(e) = validate_jsonl(&path, 200) {
        diagnoses.push(Diagnosis::new(
            DiagnosisKind::RouteCorrectionsJsonlCorrupt,
            format!("route-corrections.jsonl has corrupt lines: {e}"),
            serde_json::json!({
                "path": path.display().to_string()
            }),
        ));
    }

    diagnoses
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

/// Run all detection functions and collect all diagnoses.
pub fn run_all_detections() -> Vec<Diagnosis> {
    let mut all = Vec::new();
    all.extend(detect_plugins());
    all.extend(detect_uc());
    all.extend(detect_gitnexus());
    all.extend(detect_council_artifacts());
    all.extend(detect_circuit_breaker());
    all.extend(detect_telemetry_health());
    all.extend(detect_route_corrections());
    all
}
