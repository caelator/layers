//! Route-corrections.jsonl reader for routing weight adjustments.
//!
//! This module reads `~/.layers/route-corrections.jsonl` (written by [`emit_failure()`]
//! in [`crate::feedback`]) and exposes per-route weight adjustments derived from
//! failure records.
//!
//! Weight adjustments:
//!   - Hard failure  → route weight −0.5
//!   - Soft failure  → route weight −0.2
//!   - Correction    → chosen route −0.3, human-chosen route +0.4
//!
//! Weights are additive across multiple failure records.

use crate::feedback::{
    RouteFailure, RouteId, load_route_weights, read_recent_failures, route_corrections_path,
};
use std::collections::HashMap;
use std::path::Path;

/// A route correction derived from a single [`RouteFailure`] record.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum RouteCorrection {
    /// Hard failure: subprocess error, timeout, parse error — strongest penalty.
    Hard {
        route_id: RouteId,
        error_kind: String,
        tool_name: String,
        query: String,
    },
    /// Soft failure: output was valid but wrong or insufficient — moderate penalty.
    Soft {
        route_id: RouteId,
        error_kind: String,
        query: String,
    },
    /// Human correction: human overrode the routing decision.
    Correction {
        route_id: RouteId,
        correction: f32,
        query: String,
    },
}

#[allow(unused)]
impl RouteCorrection {
    /// Build a `RouteCorrection` from a [`RouteFailure`].
    fn from_failure(failure: &RouteFailure) -> Option<Self> {
        use crate::feedback::FailureKind;
        match &failure.failure {
            FailureKind::Hard {
                error_kind,
                tool_name,
                ..
            } => Some(RouteCorrection::Hard {
                route_id: failure.route_chosen,
                error_kind: error_kind.as_str().to_string(),
                tool_name: tool_name.clone(),
                query: failure.query_text.clone(),
            }),
            FailureKind::Soft { error_kind, .. } => Some(RouteCorrection::Soft {
                route_id: failure.route_chosen,
                error_kind: error_kind.as_str().to_string(),
                query: failure.query_text.clone(),
            }),
            FailureKind::Correction { human_chose: _, .. } => {
                // correction value: −0.3 for the wrong route, +0.4 for the human's choice
                // We represent the correction on the route that was chosen (and was wrong).
                Some(RouteCorrection::Correction {
                    route_id: failure.route_chosen,
                    correction: -0.3,
                    query: failure.query_text.clone(),
                })
            }
        }
    }

    /// The weight delta contributed by this correction.
    pub fn weight_delta(&self) -> f32 {
        match self {
            RouteCorrection::Hard { .. } => -0.5,
            RouteCorrection::Soft { .. } => -0.2,
            RouteCorrection::Correction { correction, .. } => *correction,
        }
    }
}

/// Reader for `~/.layers/route-corrections.jsonl`.
///
/// Provides:
///   - Corrections for a given `RouteId`
///   - Corrections for a given `Query` (matched by fingerprint)
///   - Raw per-route weight map (computed via [`load_route_weights`])
#[allow(dead_code)]
pub struct RouteCorrectionReader {
    failures: Vec<RouteFailure>,
}

#[allow(unused)]
impl RouteCorrectionReader {
    /// Construct a new reader by loading all records from the corrections file.
    /// Returns a reader with an empty cache if the file does not exist.
    pub fn new() -> Self {
        Self::from_path(&route_corrections_path())
    }

    /// Construct a reader from an explicit path (useful for testing).
    pub fn from_path(path: &Path) -> Self {
        let failures = read_recent_failures(path, usize::MAX);
        Self { failures }
    }

    /// Returns all corrections as parsed [`RouteCorrection`] enum variants.
    pub fn corrections(&self) -> Vec<RouteCorrection> {
        self.failures
            .iter()
            .filter_map(RouteCorrection::from_failure)
            .collect()
    }

    /// Returns corrections that apply to a specific `route_id`.
    pub fn for_route(&self, route_id: RouteId) -> Vec<RouteCorrection> {
        self.failures
            .iter()
            .filter(|f| f.route_chosen == route_id)
            .filter_map(RouteCorrection::from_failure)
            .collect()
    }

    /// Returns corrections for entries whose query fingerprint matches the given query.
    /// Uses SHA-256 fingerprint matching for consistency with [`RouteFailure::fingerprint`].
    pub fn for_query(&self, query: &str) -> Vec<RouteCorrection> {
        let fp = RouteFailure::fingerprint(query);
        self.failures
            .iter()
            .filter(|f| f.query_fingerprint == fp)
            .filter_map(RouteCorrection::from_failure)
            .collect()
    }

    /// Returns the per-route weight adjustment map.
    ///
    /// Keys are [`RouteId`] variants; values are the cumulative weight delta:
    ///   - Hard failure  → −0.5 per occurrence
    ///   - Soft failure  → −0.2 per occurrence
    ///   - Correction    → chosen route −0.3, human-chosen route +0.4
    pub fn route_weights(&self) -> HashMap<RouteId, f32> {
        load_route_weights(&self.failures)
    }

    /// Returns the weight adjustment for a specific `route_id`.
    pub fn weight_for(&self, route_id: RouteId) -> f32 {
        self.route_weights().get(&route_id).copied().unwrap_or(0.0)
    }

    /// Number of failure records loaded.
    pub fn len(&self) -> usize {
        self.failures.len()
    }

    /// True if no failure records were loaded.
    pub fn is_empty(&self) -> bool {
        self.failures.is_empty()
    }
}

impl Default for RouteCorrectionReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feedback::{
        FailureKind, HardErrorKind, RouteFailure, RoutingSignals, SoftErrorKind,
    };
    use tempfile::TempDir;

    fn temp_failure_path() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("route-corrections.jsonl");
        (dir, path)
    }

    fn write_failure(path: &std::path::Path, failure: &RouteFailure) {
        use std::io::Write;
        let line = serde_json::to_string(failure).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "{line}").unwrap();
    }

    #[test]
    fn reader_loads_from_empty_file() {
        let (dir, path) = temp_failure_path();
        let reader = RouteCorrectionReader::from_path(&path);
        assert!(reader.is_empty());
        assert!(reader.corrections().is_empty());
        drop(dir);
    }

    #[test]
    fn reader_loads_from_nonexistent_path() {
        let reader =
            RouteCorrectionReader::from_path(std::path::Path::new("/nonexistent/path.jsonl"));
        assert!(reader.is_empty());
    }

    #[test]
    fn corrections_parses_hard_failure() {
        let (_dir, path) = temp_failure_path();
        let failure = RouteFailure::new(
            "How do I implement a tokio JoinSet?".to_string(),
            RouteId::CouncilOnly,
            FailureKind::Hard {
                error_kind: HardErrorKind::Timeout,
                error_code: Some(124),
                tool_name: "gemini".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &failure);

        let reader = RouteCorrectionReader::from_path(&path);
        let corrections = reader.corrections();
        assert_eq!(corrections.len(), 1);

        match &corrections[0] {
            RouteCorrection::Hard {
                route_id,
                error_kind,
                tool_name,
                query,
            } => {
                assert_eq!(*route_id, RouteId::CouncilOnly);
                assert_eq!(error_kind.as_str(), "timeout");
                assert_eq!(tool_name.as_str(), "gemini");
                assert!(query.contains("JoinSet"));
            }
            other => panic!("expected Hard, got {other:?}"),
        }
    }

    #[test]
    fn corrections_parses_soft_failure() {
        let (_dir, path) = temp_failure_path();
        let failure = RouteFailure::new(
            "Deploy the service".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::Hallucination,
                flagged_by: "solution_scout".to_string(),
                affected_stage: "deliberation".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &failure);

        let reader = RouteCorrectionReader::from_path(&path);
        let corrections = reader.corrections();
        assert_eq!(corrections.len(), 1);

        match &corrections[0] {
            RouteCorrection::Soft {
                route_id,
                error_kind,
                query,
            } => {
                assert_eq!(*route_id, RouteId::Both);
                assert_eq!(error_kind.as_str(), "hallucination");
                assert!(query.contains("Deploy"));
            }
            other => panic!("expected Soft, got {other:?}"),
        }
    }

    #[test]
    fn corrections_parses_correction() {
        let (_dir, path) = temp_failure_path();
        let failure = RouteFailure::new(
            "Implement the refactor".to_string(),
            RouteId::CouncilOnly,
            FailureKind::Correction {
                human_chose: RouteId::CouncilWithGraph,
                reason: "council hallucinated".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &failure);

        let reader = RouteCorrectionReader::from_path(&path);
        let corrections = reader.corrections();
        assert_eq!(corrections.len(), 1);

        match &corrections[0] {
            RouteCorrection::Correction {
                route_id,
                correction,
                query,
            } => {
                assert_eq!(*route_id, RouteId::CouncilOnly);
                assert_eq!(*correction, -0.3);
                assert!(query.contains("refactor"));
            }
            other => panic!("expected Correction, got {other:?}"),
        }
    }

    #[test]
    fn corrections_for_route_filters_correctly() {
        let (_dir, path) = temp_failure_path();
        let f1 = RouteFailure::new(
            "task 1".to_string(),
            RouteId::CouncilOnly,
            FailureKind::Hard {
                error_kind: HardErrorKind::Timeout,
                error_code: Some(124),
                tool_name: "gemini".to_string(),
            },
            RoutingSignals::default(),
        );
        let f2 = RouteFailure::new(
            "task 2".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::Hallucination,
                flagged_by: "solution_scout".to_string(),
                affected_stage: "deliberation".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &f1);
        write_failure(&path, &f2);

        let reader = RouteCorrectionReader::from_path(&path);
        let council_only = reader.for_route(RouteId::CouncilOnly);
        assert_eq!(council_only.len(), 1);
        assert!(matches!(council_only[0], RouteCorrection::Hard { .. }));

        let both = reader.for_route(RouteId::Both);
        assert_eq!(both.len(), 1);
        assert!(matches!(both[0], RouteCorrection::Soft { .. }));

        let graph_only = reader.for_route(RouteId::GraphOnly);
        assert!(graph_only.is_empty());
    }

    #[test]
    fn corrections_for_query_uses_fingerprint() {
        let (_dir, path) = temp_failure_path();
        let f1 = RouteFailure::new(
            "How do I implement a tokio JoinSet?".to_string(),
            RouteId::CouncilOnly,
            FailureKind::Hard {
                error_kind: HardErrorKind::Timeout,
                error_code: Some(124),
                tool_name: "gemini".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &f1);

        let reader = RouteCorrectionReader::from_path(&path);
        // Exact same query text should match
        let matches = reader.for_query("How do I implement a tokio JoinSet?");
        assert_eq!(matches.len(), 1);
        // Different wording should not match
        let no_match = reader.for_query("How do I use tokio JoinSet?");
        assert!(no_match.is_empty());
    }

    #[test]
    fn route_weights_hard_failure_minus_point_five() {
        let (_dir, path) = temp_failure_path();
        let failure = RouteFailure::new(
            "task".to_string(),
            RouteId::CouncilOnly,
            FailureKind::Hard {
                error_kind: HardErrorKind::Timeout,
                error_code: Some(124),
                tool_name: "gemini".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &failure);

        let reader = RouteCorrectionReader::from_path(&path);
        assert_eq!(reader.weight_for(RouteId::CouncilOnly), -0.5);
        assert_eq!(reader.weight_for(RouteId::Both), 0.0);
    }

    #[test]
    fn route_weights_soft_failure_minus_point_two() {
        let (_dir, path) = temp_failure_path();
        let failure = RouteFailure::new(
            "task".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::Hallucination,
                flagged_by: "solution_scout".to_string(),
                affected_stage: "deliberation".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &failure);

        let reader = RouteCorrectionReader::from_path(&path);
        assert_eq!(reader.weight_for(RouteId::Both), -0.2);
    }

    #[test]
    fn route_weights_correction_demotes_and_boosts() {
        let (_dir, path) = temp_failure_path();
        let failure = RouteFailure::new(
            "task".to_string(),
            RouteId::CouncilOnly,
            FailureKind::Correction {
                human_chose: RouteId::CouncilWithGraph,
                reason: "wrong".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &failure);

        let reader = RouteCorrectionReader::from_path(&path);
        // The wrong route (CouncilOnly) gets −0.3
        assert_eq!(reader.weight_for(RouteId::CouncilOnly), -0.3);
        // The human's correct choice (CouncilWithGraph) gets +0.4
        assert_eq!(reader.weight_for(RouteId::CouncilWithGraph), 0.4);
    }

    #[test]
    fn route_weights_compounds_multiple_failures() {
        let (_dir, path) = temp_failure_path();
        for _ in 0..3 {
            let failure = RouteFailure::new(
                "task".to_string(),
                RouteId::CouncilOnly,
                FailureKind::Hard {
                    error_kind: HardErrorKind::Timeout,
                    error_code: Some(124),
                    tool_name: "gemini".to_string(),
                },
                RoutingSignals::default(),
            );
            write_failure(&path, &failure);
        }

        let reader = RouteCorrectionReader::from_path(&path);
        // 3 hard failures: 3 × −0.5 = −1.5
        assert_eq!(reader.weight_for(RouteId::CouncilOnly), -1.5);
    }

    #[test]
    fn weight_delta_returns_correct_values() {
        let (_dir, path) = temp_failure_path();
        let hard = RouteFailure::new(
            "task".to_string(),
            RouteId::Both,
            FailureKind::Hard {
                error_kind: HardErrorKind::NonZeroExit,
                error_code: Some(1),
                tool_name: "claude".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &hard);

        let soft = RouteFailure::new(
            "task2".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::StaleContext,
                flagged_by: "human_review".to_string(),
                affected_stage: "retrieval".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &soft);

        let corr = RouteFailure::new(
            "task3".to_string(),
            RouteId::Both,
            FailureKind::Correction {
                human_chose: RouteId::MemoryOnly,
                reason: "wrong route".to_string(),
            },
            RoutingSignals::default(),
        );
        write_failure(&path, &corr);

        let reader = RouteCorrectionReader::from_path(&path);
        let corrections = reader.corrections();

        let hard_delta = corrections
            .iter()
            .find(|c| matches!(c, RouteCorrection::Hard { .. }))
            .map(|c| c.weight_delta());
        assert_eq!(hard_delta, Some(-0.5));

        let soft_delta = corrections
            .iter()
            .find(|c| matches!(c, RouteCorrection::Soft { .. }))
            .map(|c| c.weight_delta());
        assert_eq!(soft_delta, Some(-0.2));

        let corr_delta = corrections
            .iter()
            .find(|c| matches!(c, RouteCorrection::Correction { .. }))
            .map(|c| c.weight_delta());
        assert_eq!(corr_delta, Some(-0.3));
    }
}
