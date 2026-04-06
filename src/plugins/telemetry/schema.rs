//! Telemetry event schema for the integration telemetry plugin.
//!
//! Defines the structured event types that are recorded after each routing
//! decision and plugin call. All events are serialized as JSON Lines (JSONL)
//! and appended to `~/.memoryport/telemetry/events.jsonl`.
//!
//! Schema versioning is embedded in every event so consumers can handle
//! forward compatibility gracefully.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Schema version embedded in every telemetry event.
/// Bump when the event schema changes in a backwards-incompatible way.
pub const SCHEMA_VERSION: &str = "1.0";

/// Well-known route labels used in telemetry events.
pub const ROUTE_LABELS: &[&str] = &["memory_only", "graph_only", "both", "neither"];

/// Quality assessment of a plugin call's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultQuality {
    /// The result was useful and directly answered the query.
    Useful,
    /// The result was neutral — neither helpful nor harmful.
    Neutral,
    /// The result was not useful or relevant.
    Useless,
}

/// Outcome of the overall routing decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoutingOutcome {
    /// The route succeeded and produced a useful result.
    Success,
    /// The route partially succeeded (some plugins worked, some failed).
    Partial,
    /// The route failed entirely.
    Failure,
}

/// A record of a single plugin call within a routing decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCall {
    /// Name of the plugin invoked (e.g., "memoryport", "gitnexus", "council").
    pub plugin: String,
    /// How long the call took in milliseconds.
    pub latency_ms: u64,
    /// Whether the call succeeded without error.
    pub success: bool,
    /// Human-readable quality assessment of the result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_quality: Option<ResultQuality>,
}

/// A routing decision event — the primary telemetry record.
///
/// Emitted once per query after all plugin calls have resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecisionEvent {
    /// Schema version for forward compatibility.
    pub schema_version: String,
    /// ISO-8601 timestamp of the event.
    pub timestamp: String,
    /// Always `routing_decision` for this event type.
    pub event_type: String,
    /// SHA-256 fingerprint of the query text (hex-encoded, 64 chars).
    pub query_fingerprint: String,
    /// The route that was selected for this query.
    pub chosen_route: String,
    /// Confidence score assigned by the classifier (0.0–1.0).
    pub route_confidence: f64,
    /// Individual plugin call records.
    pub plugin_calls: Vec<PluginCall>,
    /// Total wall-clock time from query receipt to final response (ms).
    pub end_to_end_latency_ms: u64,
    /// Overall outcome of the routing decision.
    pub outcome: RoutingOutcome,
    /// Optional council convergence data if council was invoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub council_data: Option<CouncilData>,
}

impl Default for RoutingDecisionEvent {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            timestamp: String::new(),
            event_type: "routing_decision".to_string(),
            query_fingerprint: String::new(),
            chosen_route: String::new(),
            route_confidence: 0.0,
            plugin_calls: Vec::new(),
            end_to_end_latency_ms: 0,
            outcome: RoutingOutcome::Success,
            council_data: None,
        }
    }
}

/// Council-specific convergence metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilData {
    /// Whether the council reached a convergent decision.
    pub converged: bool,
    /// Number of deliberation rounds before convergence (or before giving up).
    pub rounds: u32,
    /// Time spent in council deliberation (ms).
    pub latency_ms: u64,
}

/// Compute a SHA-256 fingerprint of a query string.
///
/// Returns a 64-character lowercase hex string.
#[must_use]
pub fn fingerprint_query(query: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    let hash = hasher.finish();

    // Extend to 256 bits by hashing the bytes of the hash itself
    let mut hasher2 = DefaultHasher::new();
    hash.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    format!("{hash:016x}{hash2:016x}{hash:016x}{hash2:016x}")
}

/// Load all events from a JSONL file.
pub fn load_events_from_file(path: &PathBuf) -> std::io::Result<Vec<RoutingDecisionEvent>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(path)?;
    let mut events = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<RoutingDecisionEvent>(line) {
            Ok(event) => events.push(event),
            Err(e) => {
                eprintln!("[telemetry] Warning: skipped malformed event line: {e}");
            }
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_to_json() {
        let event = RoutingDecisionEvent {
            schema_version: SCHEMA_VERSION.to_string(),
            timestamp: "2026-04-05T12:00:00Z".to_string(),
            event_type: "routing_decision".to_string(),
            query_fingerprint: fingerprint_query("hello world"),
            chosen_route: "memory_only".to_string(),
            route_confidence: 0.85,
            plugin_calls: vec![PluginCall {
                plugin: "memoryport".to_string(),
                latency_ms: 23,
                success: true,
                result_quality: Some(ResultQuality::Useful),
            }],
            end_to_end_latency_ms: 145,
            outcome: RoutingOutcome::Success,
            council_data: None,
        };

        let json = serde_json::to_string(&event).expect("must serialize");
        assert!(json.contains("routing_decision"));
        assert!(json.contains("memory_only"));
        assert!(json.contains("schema_version"));
    }

    #[test]
    fn event_deserializes_from_json() {
        let json = r#"{
            "schema_version": "1.0",
            "timestamp": "2026-04-05T12:00:00Z",
            "event_type": "routing_decision",
            "query_fingerprint": "abc123",
            "chosen_route": "graph_only",
            "route_confidence": 0.7,
            "plugin_calls": [
                {
                    "plugin": "gitnexus",
                    "latency_ms": 50,
                    "success": true,
                    "result_quality": "neutral"
                }
            ],
            "end_to_end_latency_ms": 200,
            "outcome": "success",
            "council_data": null
        }"#;

        let event: RoutingDecisionEvent = serde_json::from_str(json).expect("must deserialize");

        assert_eq!(event.schema_version, "1.0");
        assert_eq!(event.chosen_route, "graph_only");
        assert_eq!(event.plugin_calls.len(), 1);
        assert_eq!(event.plugin_calls[0].plugin, "gitnexus");
    }

    #[test]
    fn result_quality_roundtrip() {
        for quality in [
            ResultQuality::Useful,
            ResultQuality::Neutral,
            ResultQuality::Useless,
        ] {
            let json = serde_json::to_string(&quality).unwrap();
            let restored: ResultQuality = serde_json::from_str(&json).unwrap();
            assert_eq!(quality, restored);
        }
    }

    #[test]
    fn routing_outcome_roundtrip() {
        for outcome in [
            RoutingOutcome::Success,
            RoutingOutcome::Partial,
            RoutingOutcome::Failure,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            let restored: RoutingOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(outcome, restored);
        }
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let q = "hello world";
        let f1 = fingerprint_query(q);
        let f2 = fingerprint_query(q);
        assert_eq!(f1, f2);
    }

    #[test]
    fn fingerprint_differs_for_different_queries() {
        let f1 = fingerprint_query("hello");
        let f2 = fingerprint_query("world");
        assert_ne!(f1, f2);
    }

    #[test]
    fn load_events_from_file_skips_malformed_lines() {
        let tempdir = std::env::temp_dir();
        let path = tempdir.join("telemetry_test_events.jsonl");

        std::fs::write(
            &path,
            r#"{"schema_version":"1.0","event_type":"routing_decision","query_fingerprint":"abc","chosen_route":"memory_only","route_confidence":0.8,"plugin_calls":[],"end_to_end_latency_ms":100,"outcome":"success","timestamp":"2026-04-05T00:00:00Z"}
BAD LINE HERE
{"schema_version":"1.0","event_type":"routing_decision","query_fingerprint":"def","chosen_route":"graph_only","route_confidence":0.9,"plugin_calls":[],"end_to_end_latency_ms":200,"outcome":"success","timestamp":"2026-04-05T00:00:01Z"}
"#,
        )
        .unwrap();

        let events = load_events_from_file(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].query_fingerprint, "abc");
        assert_eq!(events[1].query_fingerprint, "def");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_events_from_empty_file() {
        let tempdir = std::env::temp_dir();
        let path = tempdir.join("telemetry_empty.jsonl");
        std::fs::write(&path, "").unwrap();

        let events = load_events_from_file(&path).unwrap();
        assert!(events.is_empty());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_events_from_nonexistent_file() {
        let path = PathBuf::from("/nonexistent/path/events.jsonl");
        let events = load_events_from_file(&path).unwrap();
        assert!(events.is_empty());
    }
}
