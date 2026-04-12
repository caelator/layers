//! Learning module for the technician.
//!
//! Aggregates failure patterns, enriches diagnoses with `GitNexus` execution-flow
//! context, retrieves similar past failures from `MemoryPort`, and updates routing
//! weights based on failure history.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::router::record_correction;
use crate::technician::data::TechnicianLearning;

// ---------------------------------------------------------------------------
// GitNexus enrichment
// ---------------------------------------------------------------------------

/// Result of enriching a diagnosis with `GitNexus` context.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitNexusEnrichment {
    /// Related execution flows from the knowledge graph.
    #[serde(default)]
    pub related_flows: Vec<RelatedFlow>,
    /// Recent commits affecting the diagnosis area.
    #[serde(default)]
    pub recent_changes: Vec<RecentChange>,
    /// Blast-radius affected symbols.
    #[serde(default)]
    pub affected_symbols: Vec<String>,
}

/// A related execution flow returned by `GitNexus` query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedFlow {
    pub name: String,
    pub relevance: f64,
    pub steps: Vec<String>,
}

/// A recent commit affecting the diagnosis area.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentChange {
    pub sha: String,
    pub message: String,
    pub files: Vec<String>,
}

/// Attempt to enrich a diagnosis with `GitNexus` execution-flow context.
/// Returns None if `GitNexus` is unavailable or the query fails.
/// Calls: `gitnexus query -r layers <diagnosis>`
pub fn enrich_with_gitnexus(diagnosis: &str) -> Option<GitNexusEnrichment> {
    let gitnexus_path = std::env::var("GITNEXUS_CLI")
        .ok()
        .and_then(|p| std::path::Path::new(&p).is_file().then_some(p))
        .unwrap_or_else(|| "/Users/bri/.local/bin/gitnexus".into());

    if !std::path::Path::new(&gitnexus_path).exists() {
        return None;
    }

    // Query for the diagnosis
    let query = format!("{diagnosis} failure repair council");
    let output = Command::new(&gitnexus_path)
        .args(["query", "-r", "layers", "-l", "3", &query])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse the JSON output from gitnexus query
    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let mut enrichment = GitNexusEnrichment::default();

    // Extract related processes
    if let Some(processes) = parsed.get("processes").and_then(|p| p.as_array()) {
        for proc in processes.iter().take(5) {
            let name = proc.get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let relevance = proc.get("relevance")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            let steps: Vec<String> = proc
                .get("symbols")
                .and_then(|s| s.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();

            enrichment.related_flows.push(RelatedFlow { name, relevance, steps });
        }
    }

    Some(enrichment)
}

/// Attempt to get blast-radius impact for a given symbol via `GitNexus`.
/// Returns the list of affected symbols.
#[allow(dead_code)]
pub fn get_impacted_symbols(symbol: &str) -> Vec<String> {
    let gitnexus_path = std::env::var("GITNEXUS_CLI")
        .ok()
        .and_then(|p| std::path::Path::new(&p).is_file().then_some(p))
        .unwrap_or_else(|| "/Users/bri/.local/bin/gitnexus".into());

    if !std::path::Path::new(&gitnexus_path).exists() {
        return Vec::new();
    }

    let Ok(output) = Command::new(&gitnexus_path)
        .args(["impact", "-r", "layers", symbol])
        .output()
    else {
        return Vec::new()
    };

    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut symbols = Vec::new();
    if let Some(by_depth) = parsed.get("byDepth").and_then(|b| b.as_array()) {
        for depth_group in by_depth {
            if let Some(items) = depth_group.get("items").and_then(|i| i.as_array()) {
                for item in items {
                    if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                        symbols.push(name.to_string());
                    }
                }
            }
        }
    }

    symbols
}

// ---------------------------------------------------------------------------
// MemoryPort historical failure retrieval
// ---------------------------------------------------------------------------

/// A historical failure record retrieved from `MemoryPort` curated memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalFailure {
    pub diagnosis: String,
    pub resolution: String,
    pub summary: String,
    pub ts: String,
}

/// Attempt to find similar past failures from `MemoryPort`'s curated memory.
/// Returns up to 3 most relevant historical failures.
pub fn find_similar_failures(diagnosis: &str) -> Vec<HistoricalFailure> {
    let Some(memory_path) = memoryport_memory_path() else {
        return Vec::new()
    };
    if !memory_path.exists() {
        return Vec::new();
    }

    let Ok(content) = fs::read_to_string(&memory_path) else {
        return Vec::new()
    };

    // Simple keyword matching — look for records mentioning the diagnosis kind
    let diagnosis_keywords: Vec<&str> = diagnosis.split('_').collect();
    let mut candidates: Vec<HistoricalFailure> = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(record) = serde_json::from_str::<serde_json::Value>(line) {
            let text = record.to_string().to_lowercase();
            let match_count = diagnosis_keywords
                .iter()
                .filter(|kw| text.contains(&kw.to_lowercase()))
                .count();

            if match_count > 0 {
                let summary = record
                    .get("summary")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let ts = record
                    .get("ts")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let diagnosis_field = record
                    .get("diagnosis")
                    .or_else(|| record.get("kind"))
                    .and_then(|k| k.as_str())
                    .unwrap_or("")
                    .to_string();

                candidates.push(HistoricalFailure {
                    diagnosis: diagnosis_field,
                    resolution: record
                        .get("resolution")
                        .or_else(|| record.get("outcome"))
                        .and_then(|o| o.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    summary,
                    ts,
                });
            }
        }
    }

    // Return top 3
    candidates.truncate(3);
    candidates
}

/// Path to `MemoryPort`'s curated memory JSONL.
fn memoryport_memory_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".memoryport/curated-memory.jsonl"))
}

/// Record a new learning entry in `MemoryPort`'s curated memory.
/// This is called when a repair succeeds or fails, to build failure history.
#[allow(dead_code)]
pub fn record_learning(
    failure_class: &str,
    diagnosis: &str,
    repair_outcome: &str,
    summary: &str,
) -> anyhow::Result<()> {
    let path = memoryport_memory_path()
        .unwrap_or_else(|| PathBuf::from("/tmp/curated-memory.jsonl"));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let record = serde_json::json!({
        "kind": "technician_learning",
        "ts": chrono::Utc::now().to_rfc3339(),
        "failure_class": failure_class,
        "diagnosis": diagnosis,
        "outcome": repair_outcome,
        "summary": summary,
    });

    let line = serde_json::to_string(&record)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(&mut file, "{line}")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Route-correction feedback from failure history
// ---------------------------------------------------------------------------

/// Record a route correction derived from a failed routing prediction.
/// When the technician diagnoses that a wrong route was taken, it calls this
/// to inform future routing decisions.
///
/// Note: `RouteCorrection` uses the Layer's Route enum (`Neither`, `MemoryOnly`, etc.).
/// We record a correction with predicted=Neither when the failure suggests
/// the routing heuristic was wrong — this demotes that task class.
#[allow(dead_code)]
pub fn record_routing_feedback(
    task: &str,
    _predicted_route: &str,
    _actual_route: &str,
) -> anyhow::Result<()> {
    // Record a correction with Neither as the predicted route.
    // The routing system will pick up this correction and reduce confidence
    // for similar tasks. This is a heuristic approach — the task field
    // contains the failure context, and the correction acts as a demotion signal.
    let correction = crate::router::RouteCorrection::new(
        task.to_string(),
        crate::router::Route::Neither,
        crate::router::Route::Neither,
    );

    record_correction(&correction)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Learning aggregation — update TechnicianLearning from healing records
// ---------------------------------------------------------------------------

/// Update the technician learning state from a healing record.
/// Called after each repair outcome is determined.
#[allow(dead_code)]
pub fn update_learning_from_healing(
    learning: &mut TechnicianLearning,
    failure_class: &str,
    auto_repaired: bool,
    task_context: Option<&str>,
) {
    learning.record_outcome(failure_class, auto_repaired);

    // Adjust route weights based on failure patterns
    // If a certain diagnosis is associated with a specific route being wrong,
    // reduce the weight for that route for similar tasks
    if let Some(task) = task_context {
        if !auto_repaired {
            // Route correction: demote the route that was predicted
            // This is a heuristic: when an error occurs, the predicted route might be wrong
            if let Some(route) = infer_route_from_diagnosis(failure_class) {
                let _ = record_routing_feedback(task, route, "none");
            }
        }
    }
}

/// Infer which route might be wrong based on failure class.
#[allow(dead_code)]
fn infer_route_from_diagnosis(failure_class: &str) -> Option<&'static str> {
    match failure_class {
        "uc_timeout" | "uc_non_zero_exit" => Some("uc"),
        "circuit_breaker_tripped" | "jsonl_corruption" => Some("council"),
        _ => None,
    }
}
