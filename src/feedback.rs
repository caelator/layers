#![allow(dead_code)]

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
//! All records are appended to `~/.layers/route-corrections.jsonl` using
//! `StorageSafety::atomic_write` — one JSON line per record.

use anyhow::Context;
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use substrate::DefaultStorage;
use substrate::StorageSafety;
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
            time_of_day_secs: now.hour() * 3600
                + now.minute() * 60
                + now.second(),
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
    pub fn with_decision_id(mut self, id: String) -> Self {
        self.routing_decision_id = Some(id);
        self
    }

    /// Set an optional note.
    pub fn with_note(mut self, note: String) -> Self {
        self.notes = Some(note);
        self
    }
}

// ─── RouteFailureEmitter ────────────────────────────────────────────────

/// The route-corrections.jsonl file path.
/// Uses layers' standard data directory: ~/.layers/route-corrections.jsonl
fn route_corrections_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".layers")
        .join("route-corrections.jsonl")
}

/// Emit a single `RouteFailure` to the route-corrections.jsonl file atomically.
///
/// Uses `StorageSafety::atomic_write`: writes to a temp file then renames,
/// ensuring either the old file or the new record is readable — never partial.
pub fn emit_failure(failure: &RouteFailure) -> anyhow::Result<()> {
    let path = route_corrections_path();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create .layers directory")?;
    }

    // Serialize the failure record as a JSON line
    let line = serde_json::to_string(failure)
        .context("failed to serialize RouteFailure")?;
    let data = line.into_bytes();

    // Atomic write: StorageSafety handles rename + fsync
    <DefaultStorage as StorageSafety>::atomic_write(&path, &data)?;

    Ok(())
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
}
