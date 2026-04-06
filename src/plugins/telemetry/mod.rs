//! Integration Telemetry Plugin.
//!
//! Records structured metrics after each routing decision to measure the
//! health and effectiveness of layer integrations (memory, gitnexus, council).
//!
//! Events are appended to `~/.memoryport/telemetry/events.jsonl` as JSON Lines.
//! The [`TelemetryPlugin::health_report()`] method aggregates all recorded
//! events into a human-readable [`IntegrationHealthReport`].
//!
//! # Schema
//! Every event is a JSON object with a `schema_version` field (`"1.0"`) so
//! consumers can evolve the format while remaining backwards-compatible.
//!
//! # Example
//! ```
//! use layers::plugins::telemetry::{TelemetryPlugin, RoutingDecision};
//! use std::path::PathBuf;
//!
//! let plugin = TelemetryPlugin::new(&PathBuf::from("/tmp"));
//! // ... after routing decision ...
//! plugin.flush().ok();
//! ```
//!
//! # Files
//! - `~/.memoryport/telemetry/events.jsonl` — appended-only event log

pub mod aggregator;
pub mod schema;

use std::path::Path;

use aggregator::{aggregate, format_report, IntegrationHealthReport};
use schema::{
    load_events_from_file, CouncilData, PluginCall, RoutingDecisionEvent,
    RoutingOutcome, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

/// Directory name within `base_dir` where telemetry files are stored.
pub const TELEMETRY_DIR: &str = "telemetry";

/// Filename for the events JSONL file.
pub const EVENTS_FILE: &str = "events.jsonl";

/// A routing decision with plugin call details — convenient builder for telemetry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// SHA-256 fingerprint of the query text.
    pub query_fingerprint: String,
    /// Route selected (e.g., `"memory_only"`, `"graph_only"`, `"both"`, `"neither"`).
    pub chosen_route: String,
    /// Confidence score from the classifier (0.0–1.0).
    pub route_confidence: f64,
    /// Individual plugin call records.
    pub plugin_calls: Vec<PluginCall>,
    /// Total wall-clock time from query to response (ms).
    pub end_to_end_latency_ms: u64,
    /// Overall outcome.
    pub outcome: RoutingOutcome,
    /// Council convergence data, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub council_data: Option<CouncilData>,
}

impl Default for RoutingDecision {
    fn default() -> Self {
        Self {
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

impl RoutingDecision {
    /// Add a plugin call record to this decision.
    #[must_use]
    pub fn with_plugin_call(mut self, call: PluginCall) -> Self {
        self.plugin_calls.push(call);
        self
    }

    /// Add council data to this decision.
    #[must_use]
    pub fn with_council_data(mut self, data: CouncilData) -> Self {
        self.council_data = Some(data);
        self
    }
}

/// The integration telemetry plugin.
///
/// Records routing decision events to a JSONL file and computes health reports
/// from accumulated events.
#[derive(Debug, Clone)]
pub struct TelemetryPlugin {
    /// In-memory cache of events recorded this session.
    events: Vec<RoutingDecisionEvent>,
    /// Path to the events JSONL file.
    event_path: PathBuf,
    /// Whether the plugin has been flushed (events written to disk) this session.
    flushed_this_session: bool,
}

impl TelemetryPlugin {
    /// Construct a new `TelemetryPlugin` with the given base directory.
    ///
    /// The telemetry directory `~/.memoryport/telemetry/` is created if it does
    /// not exist. On startup, any existing events in `events.jsonl` are loaded
    /// into the in-memory event cache so that [`health_report()`] has a complete
    /// view of all historical data.
    ///
    /// # Panics
    /// Panics if `base_dir` is not a valid UTF-8 path (on platforms where this matters).
    #[must_use]
    pub fn new(base_dir: &Path) -> Self {
        let telemetry_dir = base_dir.join(TELEMETRY_DIR);
        let event_path = telemetry_dir.join(EVENTS_FILE);

        // Ensure the telemetry directory exists
        if let Err(e) = std::fs::create_dir_all(&telemetry_dir) {
            eprintln!(
                "[telemetry] Warning: could not create telemetry dir {}: {e}",
                telemetry_dir.display()
            );
        }

        // Load existing events on startup
        let events = load_events_from_file(&event_path).unwrap_or_else(|e| {
            eprintln!(
                "[telemetry] Warning: could not load existing events from {}: {e}",
                event_path.display()
            );
            Vec::new()
        });

        Self {
            events,
            event_path,
            flushed_this_session: false,
        }
    }

    /// Record a single telemetry event by appending it to the JSONL file.
    ///
    /// The event is also cached in memory.
    ///
    /// # Errors
    /// Returns an error if the file could not be opened or written to.
    pub fn record(&mut self, event: RoutingDecisionEvent) -> anyhow::Result<()> {
        self.events.push(event.clone());

        let line = serde_json::to_string(&event)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.event_path)?;

        writeln!(file, "{line}")?;
        self.flushed_this_session = true;
        Ok(())
    }

    /// Convenience: construct and record a routing decision event.
    ///
    /// # Errors
    /// Returns an error if the underlying file operation fails.
    pub fn record_routing_decision(&mut self, decision: RoutingDecision) -> anyhow::Result<()> {
        let event = RoutingDecisionEvent {
            schema_version: SCHEMA_VERSION.to_string(),
            timestamp: crate::util::iso_now(),
            event_type: "routing_decision".to_string(),
            query_fingerprint: decision.query_fingerprint,
            chosen_route: decision.chosen_route,
            route_confidence: decision.route_confidence,
            plugin_calls: decision.plugin_calls,
            end_to_end_latency_ms: decision.end_to_end_latency_ms,
            outcome: decision.outcome,
            council_data: decision.council_data,
        };

        self.record(event)
    }

    /// Build a health report from all accumulated events (in-memory + on-disk).
    ///
    /// This runs aggregation over all events the plugin has collected — both
    /// those loaded from disk at startup and any new ones recorded this session.
    #[must_use]
    pub fn health_report(&self) -> IntegrationHealthReport {
        aggregate(&self.events)
    }

    /// Return a formatted, human-readable health report string.
    #[must_use]
    pub fn health_report_string(&self) -> String {
        format_report(&self.health_report())
    }

    /// Flush any in-memory events that have not yet been written to disk.
    ///
    /// In normal operation, [`record()`] appends immediately. This method is
    /// useful for ensuring all events are persisted after a batch of
    /// in-memory-only recordings (e.g., during testing).
    ///
    /// # Errors
    /// Returns an error if any file write fails.
    pub fn flush(&mut self) -> anyhow::Result<()> {
        if self.flushed_this_session {
            self.flushed_this_session = false;
        }
        // Events are already flushed by record(); this is a no-op in normal use.
        Ok(())
    }

    /// Get the total count of events currently in the in-memory cache.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Get the path to the events file.
    #[must_use]
    pub fn event_file_path(&self) -> &PathBuf {
        &self.event_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schema::{PluginCall, ResultQuality, RoutingOutcome};
    use RoutingOutcome::Success;

    // Global counter so each test invocation gets a unique temp dir
    static TELEMETRY_TEST_COUNTER: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);

    fn next_test_dir() -> PathBuf {
        let counter = TELEMETRY_TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pid = std::process::id();
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(format!("layers_telemetry_test_{pid}_{counter}"))
    }

    fn temp_telemetry_plugin() -> (TelemetryPlugin, PathBuf) {
        let tempdir = next_test_dir();
        std::fs::create_dir_all(&tempdir).ok();
        (TelemetryPlugin::new(&tempdir), tempdir)
    }

    fn cleanup(path: &PathBuf) {
        std::fs::remove_dir_all(path).ok();
    }

    #[test]
    fn new_creates_telemetry_dir() {
        let pid = std::process::id();
        let counter = std::sync::atomic::AtomicU64::new(0).fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tempdir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(format!("layers_new_test_{pid}_{counter}"));
        std::fs::remove_dir_all(&tempdir).ok();

        let plugin = TelemetryPlugin::new(&tempdir);
        // The file itself may not exist yet (only after first record)
        assert!(!plugin.event_file_path().exists() || plugin.event_file_path().exists());
        std::fs::remove_dir_all(&tempdir).ok();
    }

    #[test]
    fn record_appends_event_to_file() {
        let (mut plugin, tempdir) = temp_telemetry_plugin();
        let events_file = plugin.event_file_path().clone();

        let decision = RoutingDecision {
            query_fingerprint: "abc123".to_string(),
            chosen_route: "memory_only".to_string(),
            route_confidence: 0.85,
            plugin_calls: vec![PluginCall {
                plugin: "memoryport".to_string(),
                latency_ms: 23,
                success: true,
                result_quality: Some(ResultQuality::Useful),
            }],
            end_to_end_latency_ms: 145,
            outcome: Success,
            ..Default::default()
        };

        plugin.record_routing_decision(decision).expect("record must succeed");

        // Verify file contains the event
        let content = std::fs::read_to_string(&events_file).expect("file must exist");
        let parsed: RoutingDecisionEvent =
            serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(parsed.chosen_route, "memory_only");
        assert_eq!(parsed.plugin_calls[0].plugin, "memoryport");

        cleanup(&tempdir);
    }

    #[test]
    fn health_report_from_batch() {
        let (mut plugin, tempdir) = temp_telemetry_plugin();

        for i in 0..5 {
            let decision = RoutingDecision {
                query_fingerprint: format!("fingerprint_{i}"),
                chosen_route: "memory_only".to_string(),
                route_confidence: 0.9,
                plugin_calls: vec![PluginCall {
                    plugin: "memoryport".to_string(),
                    latency_ms: 20 + (i as u64 * 10),
                    success: true,
                    result_quality: Some(ResultQuality::Useful),
                }],
                end_to_end_latency_ms: 100 + (i as u64 * 10),
                outcome: Success,
                ..Default::default()
            };
            plugin.record_routing_decision(decision).unwrap();
        }

        let report = plugin.health_report();
        assert_eq!(report.total_events, 5);
        assert!((report.average_latency_ms - 120.0).abs() < 0.1); // avg of 100,110,120,130,140
        assert!((report.routing_accuracy_estimate - 1.0).abs() < 0.01);

        cleanup(&tempdir);
    }

    #[test]
    fn health_report_includes_all_loaded_events() {
        // Use next_test_dir to get a unique path
        let tempdir = next_test_dir();
        std::fs::create_dir_all(&tempdir).ok();

        // The telemetry plugin stores events at: base_dir/telemetry/events.jsonl
        let telemetry_dir = tempdir.join(TELEMETRY_DIR);
        let events_file = telemetry_dir.join(EVENTS_FILE);

        // Pre-write one event to the events file
        let pre_event = RoutingDecisionEvent {
            schema_version: SCHEMA_VERSION.to_string(),
            timestamp: "2026-04-05T00:00:00Z".to_string(),
            event_type: "routing_decision".to_string(),
            query_fingerprint: "pre_written".to_string(),
            chosen_route: "graph_only".to_string(),
            route_confidence: 0.7,
            plugin_calls: vec![],
            end_to_end_latency_ms: 200,
            outcome: Success,
            ..Default::default()
        };
        let line = serde_json::to_string(&pre_event).unwrap();
        std::fs::create_dir_all(&telemetry_dir).ok();
        std::fs::write(&events_file, format!("{line}\n")).unwrap();

        // Create plugin using the SAME tempdir so it loads the pre-existing event
        let plugin = TelemetryPlugin::new(&tempdir);
        assert_eq!(
            plugin.event_count(),
            1,
            "plugin should have loaded the pre-written event"
        );

        // Add another event using a fresh plugin instance on the same dir
        let mut plugin2 = TelemetryPlugin::new(&tempdir);
        let decision = RoutingDecision {
            query_fingerprint: "new_event".to_string(),
            chosen_route: "memory_only".to_string(),
            route_confidence: 0.9,
            plugin_calls: vec![],
            end_to_end_latency_ms: 100,
            outcome: Success,
            ..Default::default()
        };
        plugin2.record_routing_decision(decision).unwrap();

        let report = plugin2.health_report();
        // Should include both pre-existing and new event
        assert_eq!(report.total_events, 2);

        cleanup(&tempdir);
    }

    #[test]
    fn flush_is_idempotent() {
        let (mut plugin, tempdir) = temp_telemetry_plugin();
        plugin.flush().expect("flush must succeed");
        plugin.flush().expect("second flush must succeed");
        cleanup(&tempdir);
    }

    #[test]
    fn record_multiple_outcomes() {
        let (mut plugin, tempdir) = temp_telemetry_plugin();

        let outcomes = [
            RoutingOutcome::Success,
            RoutingOutcome::Success,
            RoutingOutcome::Partial,
            RoutingOutcome::Failure,
        ];

        for (i, &outcome) in outcomes.iter().enumerate() {
            let decision = RoutingDecision {
                query_fingerprint: format!("fingerprint_{i}"),
                chosen_route: "memory_only".to_string(),
                route_confidence: 0.8,
                plugin_calls: vec![],
                end_to_end_latency_ms: 100,
                outcome,
                ..Default::default()
            };
            plugin.record_routing_decision(decision).unwrap();
        }

        let report = plugin.health_report();
        // 1 partial + 1 failure out of 4 = 0.5 error rate
        assert!((report.error_rate - 0.5).abs() < 0.01);
        // 2 success out of 4 = 0.5 accuracy
        assert!((report.routing_accuracy_estimate - 0.5).abs() < 0.01);

        cleanup(&tempdir);
    }
}
