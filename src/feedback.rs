//! Route-correction feedback types — RFC 006.
//!
//! ## Failure Taxonomy
//!
//! Every `RouteFailure` is classified into one of three tiers:
//!
//! - **Hard**: subprocess error, timeout, parse error, binary missing
//! - **Soft**: output was valid but wrong/insufficient (flagged by Solution Scout or human)
//! - **Correction**: human explicitly overrode the routing decision
//!
//! ## Storage
//!
//! All records are appended to `~/.layers/route-corrections.jsonl` —
//! one JSON line per record.

use anyhow::Context;
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

// ─── RouteId ────────────────────────────────────────────────────────────────

/// Identifies a routing route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteId {
    MemoryOnly,
    GraphOnly,
    Both,
    Neither,
    CouncilOnly,
    CouncilWithGraph,
    CouncilWithMemory,
}

impl RouteId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MemoryOnly => "memory_only",
            Self::GraphOnly => "graph_only",
            Self::Both => "both",
            Self::Neither => "neither",
            Self::CouncilOnly => "council_only",
            Self::CouncilWithGraph => "council_with_graph",
            Self::CouncilWithMemory => "council_with_memory",
        }
    }
}

impl std::fmt::Display for RouteId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ─── FailureKind ─────────────────────────────────────────────────────────

/// The type and cause of a routing failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tier", rename_all = "snake_case")]
pub enum FailureKind {
    /// Hard failure: subprocess error, timeout, parse error.
    Hard {
        error_kind: HardErrorKind,
        error_code: Option<u32>,
        tool_name: String,
    },
    /// Soft failure: output was valid but wrong or insufficient.
    Soft {
        error_kind: SoftErrorKind,
        /// Who flagged this: `solution_scout` | `human_review` | `test_failure`
        flagged_by: String,
        /// Which stage produced the bad output.
        affected_stage: String,
    },
    /// Human correction: human overrode the routing decision.
    Correction {
        /// What the human chose instead.
        human_chose: RouteId,
        /// Why the correction was made.
        reason: String,
    },
}

/// Kinds of hard failures — subprocess-level errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardErrorKind {
    Timeout,
    NonZeroExit,
    ParseError,
    BinaryMissing,
    ConfigMissing,
    LockContention,
}

impl HardErrorKind {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::NonZeroExit => "non_zero_exit",
            Self::ParseError => "parse_error",
            Self::BinaryMissing => "binary_missing",
            Self::ConfigMissing => "config_missing",
            Self::LockContention => "lock_contention",
        }
    }
}

/// Kinds of soft failures — quality failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftErrorKind {
    Hallucination,
    InsufficientContext,
    WrongModelForTask,
    StaleContext,
    Contradiction,
}

impl SoftErrorKind {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hallucination => "hallucination",
            Self::InsufficientContext => "insufficient_context",
            Self::WrongModelForTask => "wrong_model_for_task",
            Self::StaleContext => "stale_context",
            Self::Contradiction => "contradiction",
        }
    }
}

// ─── RoutingSignals ──────────────────────────────────────────────────────

/// Contextual signals present at the time of routing.
/// Used for feature extraction in weight adjustment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingSignals {
    pub query_length_chars: usize,
    pub history_turns: usize,
    pub intent_confidence: f64,
    pub graph_symbol_count: usize,
    pub memory_hits: usize,
    pub council_load: f64,
    pub time_of_day_secs: u32,
    pub day_of_week: u8,
    pub uc_available: bool,
    pub gitnexus_available: bool,
}

impl Default for RoutingSignals {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            query_length_chars: 0,
            history_turns: 0,
            intent_confidence: 0.0,
            graph_symbol_count: 0,
            memory_hits: 0,
            council_load: 0.0,
            time_of_day_secs: now.hour() * 3600 + now.minute() * 60 + now.second(),
            day_of_week: now.weekday().num_days_from_monday() as u8,
            uc_available: false,
            gitnexus_available: false,
        }
    }
}

// ─── RouteFailure ────────────────────────────────────────────────────────

/// A single routing failure event — the primary record in route-corrections.jsonl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteFailure {
    /// Schema version of this record. Currently "1.0".
    pub schema_version: String,
    /// Stable URN for this failure record.
    pub id: String,
    /// When the failure was detected.
    pub detected_at: DateTime<Utc>,
    /// Which routing decision this was correcting (optional — may be absent for corrections).
    pub routing_decision_id: Option<String>,
    /// The intent/query text that triggered the route.
    pub query_text: String,
    /// SHA-256 fingerprint of the normalized query (for aggregation).
    pub query_fingerprint: String,
    /// Which route was chosen by layers.
    pub route_chosen: RouteId,
    /// Failure tier and kind.
    pub failure: FailureKind,
    /// Contextual signals at time of routing.
    pub signals: RoutingSignals,
    /// Optional freeform notes.
    pub notes: Option<String>,
}

impl RouteFailure {
    /// Construct a new `RouteFailure`.
    pub fn new(
        query_text: String,
        route_chosen: RouteId,
        failure: FailureKind,
        signals: RoutingSignals,
    ) -> Self {
        Self {
            schema_version: String::from("1.0"),
            id: Uuid::new_v4().to_string(),
            detected_at: Utc::now(),
            routing_decision_id: None,
            query_text: query_text.clone(),
            query_fingerprint: Self::fingerprint(&query_text),
            route_chosen,
            failure,
            signals,
            notes: None,
        }
    }

    /// Compute a SHA-256 fingerprint of the normalized query text.
    /// Normalization: lowercase, trim, collapse whitespace.
    pub fn fingerprint(query: &str) -> String {
        let normalized: String = query
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let mut hasher = Sha256::new();
        hasher.update(normalized.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Set the routing decision ID.
    #[allow(dead_code)]
    pub fn with_decision_id(mut self, id: String) -> Self {
        self.routing_decision_id = Some(id);
        self
    }

    /// Set an optional note.
    #[allow(dead_code)]
    pub fn with_note(mut self, note: String) -> Self {
        self.notes = Some(note);
        self
    }
}

// ─── RouteFailureEmitter ────────────────────────────────────────────────

/// The route-corrections.jsonl file path.
/// Uses layers' standard data directory: ~/.layers/route-corrections.jsonl
pub fn route_corrections_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".layers")
        .join("route-corrections.jsonl")
}

/// Emit a single `RouteFailure` to the route-corrections.jsonl file.
///
/// Appends one JSON line to the JSONL file so that multiple failures compound
/// over time.  Previous versions used `atomic_write` (rename-based), which
/// **replaced** the file on every call — meaning only the most recent failure
/// was ever stored and route weights could never compound.
pub fn emit_failure(failure: &RouteFailure) -> anyhow::Result<()> {
    let path = route_corrections_path();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("failed to create .layers directory")?;
    }

    // Serialize the failure record as a JSON line
    let line = serde_json::to_string(failure).context("failed to serialize RouteFailure")?;

    // Append (not replace) — JSONL files must accumulate records.
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .context("failed to open route-corrections.jsonl for append")?;
    writeln!(file, "{line}").context("failed to write RouteFailure line")?;
    file.flush()
        .context("failed to flush route-corrections.jsonl")?;

    Ok(())
}

// ─── Route-correction reader ─────────────────────────────────────────────────

/// Read the last N `RouteFailure` records from a jsonl file.
/// Returns the most-recent entries first (reverse-chronological order).
pub fn read_recent_failures(path: &Path, limit: usize) -> Vec<RouteFailure> {
    if limit == 0 {
        return Vec::new();
    }
    let Some(content) = std::fs::read_to_string(path).ok() else {
        return Vec::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..]
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Derive per-route weight adjustments from a collection of failures.
///
/// Weight map:
/// - Hard failures on a route → demote that route (negative weight)
/// - Soft failures on a route → slight demotion (smaller negative weight)
/// - Corrections (human overrode to a different route) → demote the wrong route,
///   boost the human-chosen route
///
/// Weights are additive: multiple failures on the same route compound.
pub fn load_route_weights(failures: &[RouteFailure]) -> HashMap<RouteId, f32> {
    let mut weights: HashMap<RouteId, f32> = HashMap::new();

    for failure in failures {
        let delta = match &failure.failure {
            // Hard failures carry the strongest penalty
            FailureKind::Hard { .. } => -0.5_f32,
            // Soft failures carry a moderate penalty
            FailureKind::Soft { .. } => -0.2_f32,
            // Corrections: demote the auto-chosen route, boost what the human picked
            FailureKind::Correction { human_chose, .. } => {
                // Demote the route layers chose (it was wrong)
                *weights.entry(failure.route_chosen).or_insert(0.0) -= 0.3;
                // Boost the human's correct choice
                *weights.entry(*human_chose).or_insert(0.0) += 0.4;
                continue;
            }
        };
        *weights.entry(failure.route_chosen).or_insert(0.0) += delta;
    }

    weights
}

/// Convert a router `Route` to a `RouteId` for feedback recording.
/// Used when the router's classification is the "route chosen" in a failure record.
#[allow(dead_code)]
pub fn router_route_to_feedback_id(route: crate::router::Route) -> RouteId {
    match route {
        crate::router::Route::Neither => RouteId::Neither,
        crate::router::Route::MemoryOnly => RouteId::MemoryOnly,
        crate::router::Route::GraphOnly => RouteId::GraphOnly,
        crate::router::Route::Both => RouteId::Both,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_id_display() {
        assert_eq!(RouteId::MemoryOnly.as_str(), "memory_only");
        assert_eq!(RouteId::CouncilWithGraph.as_str(), "council_with_graph");
    }

    #[test]
    fn failure_kind_serde() {
        let hard = FailureKind::Hard {
            error_kind: HardErrorKind::Timeout,
            error_code: Some(124),
            tool_name: "uc".to_string(),
        };
        let json = serde_json::to_string(&hard).unwrap();
        assert!(json.contains("\"timeout\""));
        assert!(json.contains("\"hard\""));
    }

    #[test]
    fn route_failure_new() {
        let failure = RouteFailure::new(
            "How do I implement a tokio JoinSet?".to_string(),
            RouteId::CouncilOnly,
            FailureKind::Hard {
                error_kind: HardErrorKind::Timeout,
                error_code: Some(124),
                tool_name: "council".to_string(),
            },
            RoutingSignals::default(),
        );

        assert_eq!(failure.schema_version, "1.0");
        assert!(!failure.id.is_empty());
        assert!(failure.query_fingerprint.len() == 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn query_fingerprint_is_deterministic() {
        let q = "Implement JoinSet with tokio";
        let fp1 = RouteFailure::fingerprint(q);
        let fp2 = RouteFailure::fingerprint(q);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn query_fingerprint_normalizes_case_and_whitespace() {
        let fp1 = RouteFailure::fingerprint("  TOKIO  JoinSet  implement  ");
        let fp2 = RouteFailure::fingerprint("tokio joinSet implement");
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn route_failure_with_decision_id() {
        let mut failure = RouteFailure::new(
            "test query".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::Hallucination,
                flagged_by: "solution_scout".to_string(),
                affected_stage: "deliberation".to_string(),
            },
            RoutingSignals::default(),
        );
        assert!(failure.routing_decision_id.is_none());

        failure.routing_decision_id = Some("dec-123".to_string());
        assert_eq!(failure.routing_decision_id.as_ref().unwrap(), "dec-123");
    }

    #[test]
    fn route_failure_serde_roundtrip() {
        let failure = RouteFailure::new(
            "Implement graceful shutdown".to_string(),
            RouteId::Both,
            FailureKind::Correction {
                human_chose: RouteId::MemoryOnly,
                reason: "council hallucinated a non-existent API".to_string(),
            },
            RoutingSignals {
                query_length_chars: 28,
                history_turns: 3,
                intent_confidence: 0.3,
                graph_symbol_count: 50,
                memory_hits: 2,
                council_load: 0.1,
                time_of_day_secs: 43200,
                day_of_week: 1,
                uc_available: true,
                gitnexus_available: true,
            },
        )
        .with_decision_id("dec-abc".to_string())
        .with_note("Found via session review".to_string());

        let json = serde_json::to_string(&failure).unwrap();
        let parsed: RouteFailure = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.schema_version, "1.0");
        assert_eq!(parsed.id, failure.id);
        assert_eq!(parsed.query_fingerprint, failure.query_fingerprint);
        assert_eq!(parsed.route_chosen, RouteId::Both);
        assert_eq!(parsed.routing_decision_id.as_ref().unwrap(), "dec-abc");
        assert_eq!(parsed.notes.as_ref().unwrap(), "Found via session review");
        assert_eq!(parsed.signals.query_length_chars, 28);
    }

    #[test]
    fn routing_signals_default() {
        let sigs = RoutingSignals::default();
        // time_of_day_secs should be non-zero (default is set from current time)
        assert!(sigs.time_of_day_secs > 0);
        assert!(sigs.day_of_week <= 6);
    }

    #[test]
    fn read_recent_failures_empty_file() {
        let tmp = tempfile::NamedTempFile::with_suffix(".jsonl").unwrap();
        let path = tmp.path();
        let failures = read_recent_failures(path, 10);
        assert!(failures.is_empty());
    }

    #[test]
    fn read_recent_failures_returns_last_n_in_order() {
        let tmp = tempfile::NamedTempFile::with_suffix(".jsonl").unwrap();
        let path = tmp.path();

        let f1 = RouteFailure::new(
            "query a".to_string(),
            RouteId::Both,
            FailureKind::Hard {
                error_kind: HardErrorKind::Timeout,
                error_code: Some(124),
                tool_name: "gemini".into(),
            },
            RoutingSignals::default(),
        );
        let f2 = RouteFailure::new(
            "query b".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::Hallucination,
                flagged_by: "solution_scout".into(),
                affected_stage: "deliberation".into(),
            },
            RoutingSignals::default(),
        );
        let f3 = RouteFailure::new(
            "query c".to_string(),
            RouteId::Both,
            FailureKind::Correction {
                human_chose: RouteId::MemoryOnly,
                reason: "wrong route".into(),
            },
            RoutingSignals::default(),
        );

        // Write three separate lines to the jsonl file (each JSON object on its own line)
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        for f in &[&f1, &f2, &f3] {
            use std::io::Write;
            writeln!(file, "{}", serde_json::to_string(f).unwrap()).unwrap();
        }

        // Read last 2 — last 2 lines of the file are f2 and f3 (in that order)
        let recent = read_recent_failures(path, 2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].query_text, "query b"); // f2 is second-to-last
        assert_eq!(recent[1].query_text, "query c"); // f3 is last

        // Read last 5 (more than exist)
        let recent = read_recent_failures(path, 5);
        assert_eq!(recent.len(), 3);

        // Read exactly 1 — returns the last line (f3)
        let recent = read_recent_failures(path, 1);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].query_text, "query c");
    }

    #[test]
    fn read_recent_failures_ignores_malformed_lines() {
        let tmp = tempfile::NamedTempFile::with_suffix(".jsonl").unwrap();
        let path = tmp.path();

        let f1 = RouteFailure::new(
            "good query".to_string(),
            RouteId::Both,
            FailureKind::Hard {
                error_kind: HardErrorKind::Timeout,
                error_code: Some(124),
                tool_name: "gemini".into(),
            },
            RoutingSignals::default(),
        );
        let f2 = RouteFailure::new(
            "also good".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::Hallucination,
                flagged_by: "solution_scout".into(),
                affected_stage: "deliberation".into(),
            },
            RoutingSignals::default(),
        );

        // Write properly: each entry on its own line, with some malformed lines mixed in
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        use std::io::Write;
        // Malformed: valid JSON but not a RouteFailure
        writeln!(file, "{{\"not_a_route_failure\": true}}").unwrap();
        writeln!(file, "{}", serde_json::to_string(&f1).unwrap()).unwrap();
        // Malformed: invalid JSON (missing closing brace)
        writeln!(file, "{{\"incomplete json\"").unwrap();
        writeln!(file, "{}", serde_json::to_string(&f2).unwrap()).unwrap();

        let recent = read_recent_failures(path, 10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].query_text, "good query");
        assert_eq!(recent[1].query_text, "also good");
    }

    #[test]
    fn load_route_weights_hard_failure_demotes() {
        let failures = vec![
            RouteFailure::new(
                "task 1".to_string(),
                RouteId::Both,
                FailureKind::Hard {
                    error_kind: HardErrorKind::Timeout,
                    error_code: Some(124),
                    tool_name: "gemini".into(),
                },
                RoutingSignals::default(),
            ),
            RouteFailure::new(
                "task 2".to_string(),
                RouteId::Both,
                FailureKind::Hard {
                    error_kind: HardErrorKind::NonZeroExit,
                    error_code: Some(1),
                    tool_name: "claude".into(),
                },
                RoutingSignals::default(),
            ),
        ];

        let weights = load_route_weights(&failures);
        // Two hard failures on Both = -0.5 * 2 = -1.0
        assert_eq!(weights.get(&RouteId::Both), Some(&-1.0_f32));
        // MemoryOnly never failed, so not in map
        assert!(weights.get(&RouteId::MemoryOnly).is_none());
    }

    #[test]
    fn load_route_weights_soft_failure_demotes_less_than_hard() {
        let failures = vec![RouteFailure::new(
            "task".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::Hallucination,
                flagged_by: "solution_scout".into(),
                affected_stage: "deliberation".into(),
            },
            RoutingSignals::default(),
        )];

        let weights = load_route_weights(&failures);
        assert_eq!(weights.get(&RouteId::Both), Some(&-0.2_f32));
    }

    #[test]
    fn load_route_weights_correction_demotes_wrong_boosts_correct() {
        let failures = vec![RouteFailure::new(
            "task".to_string(),
            RouteId::Both,
            FailureKind::Correction {
                human_chose: RouteId::MemoryOnly,
                reason: "chose wrong".into(),
            },
            RoutingSignals::default(),
        )];

        let weights = load_route_weights(&failures);
        // Demote Both (the wrong choice)
        assert_eq!(weights.get(&RouteId::Both), Some(&-0.3_f32));
        // Boost MemoryOnly (the human's correct choice)
        assert_eq!(weights.get(&RouteId::MemoryOnly), Some(&0.4_f32));
    }

    #[test]
    fn load_route_weights_compounds_across_multiple_failures() {
        let failures = vec![
            RouteFailure::new(
                "t1".to_string(),
                RouteId::GraphOnly,
                FailureKind::Hard {
                    error_kind: HardErrorKind::Timeout,
                    error_code: Some(124),
                    tool_name: "gemini".into(),
                },
                RoutingSignals::default(),
            ),
            RouteFailure::new(
                "t2".to_string(),
                RouteId::GraphOnly,
                FailureKind::Hard {
                    error_kind: HardErrorKind::Timeout,
                    error_code: Some(124),
                    tool_name: "claude".into(),
                },
                RoutingSignals::default(),
            ),
            RouteFailure::new(
                "t3".to_string(),
                RouteId::Both,
                FailureKind::Soft {
                    error_kind: SoftErrorKind::Hallucination,
                    flagged_by: "solution_scout".into(),
                    affected_stage: "deliberation".into(),
                },
                RoutingSignals::default(),
            ),
        ];

        let weights = load_route_weights(&failures);
        // GraphOnly: two hard failures = -0.5 * 2 = -1.0
        assert_eq!(weights.get(&RouteId::GraphOnly), Some(&-1.0_f32));
        // Both: one soft = -0.2
        assert_eq!(weights.get(&RouteId::Both), Some(&-0.2_f32));
    }

    #[test]
    fn load_route_weights_empty_slice_returns_empty_map() {
        let weights = load_route_weights(&[]);
        assert!(weights.is_empty());
    }

    #[test]
    fn router_route_to_feedback_id_roundtrips() {
        use crate::router::Route;
        assert_eq!(
            router_route_to_feedback_id(Route::Neither),
            RouteId::Neither
        );
        assert_eq!(
            router_route_to_feedback_id(Route::MemoryOnly),
            RouteId::MemoryOnly
        );
        assert_eq!(
            router_route_to_feedback_id(Route::GraphOnly),
            RouteId::GraphOnly
        );
        assert_eq!(router_route_to_feedback_id(Route::Both), RouteId::Both);
    }
}
