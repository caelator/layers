//! Health report aggregation for the integration telemetry plugin.
//!
//! Consumes a collection of [`RoutingDecisionEvent`] records and produces
//! an [`IntegrationHealthReport`] that summarizes the health and performance
//! of each plugin as well as overall routing metrics.

use crate::plugins::telemetry::schema::{RoutingDecisionEvent, RoutingOutcome, SCHEMA_VERSION};
use std::collections::HashMap;

/// Per-plugin health metrics.
#[derive(Debug, Clone)]
pub struct PluginHealth {
    /// Total number of calls to this plugin.
    pub calls: u64,
    /// Number of calls that returned an error.
    pub errors: u64,
    /// Average latency across all successful calls (ms).
    pub avg_latency_ms: f64,
    /// Fraction of calls that succeeded (no error).
    pub success_rate: f64,
}

impl Default for PluginHealth {
    fn default() -> Self {
        Self {
            calls: 0,
            errors: 0,
            avg_latency_ms: 0.0,
            success_rate: 1.0,
        }
    }
}

/// Aggregated health report for all integrations.
#[derive(Debug, Clone)]
pub struct IntegrationHealthReport {
    /// Total number of routing decision events processed.
    pub total_events: u64,
    /// Per-plugin health breakdown.
    pub plugin_health: HashMap<String, PluginHealth>,
    /// Average end-to-end latency across all events (ms).
    pub average_latency_ms: f64,
    /// Fraction of events with outcome != success.
    pub error_rate: f64,
    /// Estimated routing accuracy: fraction of events where outcome == success.
    pub routing_accuracy_estimate: f64,
    /// Fraction of events where council was invoked and converged.
    pub council_convergence_rate: f64,
}

impl Default for IntegrationHealthReport {
    fn default() -> Self {
        Self {
            total_events: 0,
            plugin_health: HashMap::new(),
            average_latency_ms: 0.0,
            error_rate: 0.0,
            routing_accuracy_estimate: 0.0,
            council_convergence_rate: 0.0,
        }
    }
}

/// Aggregate a collection of events into a health report.
pub fn aggregate(events: &[RoutingDecisionEvent]) -> IntegrationHealthReport {
    if events.is_empty() {
        return IntegrationHealthReport::default();
    }

    let total_events = events.len() as u64;
    let mut plugin_health: HashMap<String, PluginHealth> = HashMap::new();

    // Track per-plugin totals for average computation
    let mut plugin_latency_sum: HashMap<String, u64> = HashMap::new();
    let mut total_latency_ms: u64 = 0;
    let mut success_count: u64 = 0;
    let mut partial_count: u64 = 0;
    let mut failure_count: u64 = 0;
    let mut council_converged: u64 = 0;
    let mut council_total: u64 = 0;

    for event in events {
        total_latency_ms += event.end_to_end_latency_ms;

        match event.outcome {
            RoutingOutcome::Success => success_count += 1,
            RoutingOutcome::Partial => partial_count += 1,
            RoutingOutcome::Failure => failure_count += 1,
        }

        if let Some(ref council) = event.council_data {
            council_total += 1;
            if council.converged {
                council_converged += 1;
            }
        }

        for call in &event.plugin_calls {
            let health = plugin_health.entry(call.plugin.clone()).or_default();
            health.calls += 1;

            if !call.success {
                health.errors += 1;
            }

            *plugin_latency_sum.entry(call.plugin.clone()).or_insert(0) += call.latency_ms;
        }
    }

    // Compute per-plugin average latency and success rate
    for (plugin, health) in &mut plugin_health {
        let latency_sum = plugin_latency_sum.get(plugin).copied().unwrap_or(0);
        health.avg_latency_ms = if health.calls > 0 {
            latency_sum as f64 / health.calls as f64
        } else {
            0.0
        };
        health.success_rate = if health.calls > 0 {
            (health.calls - health.errors) as f64 / health.calls as f64
        } else {
            1.0
        };
    }

    let average_latency_ms = total_latency_ms as f64 / total_events as f64;
    let error_rate = (partial_count + failure_count) as f64 / total_events as f64;
    let routing_accuracy_estimate = success_count as f64 / total_events as f64;
    let council_convergence_rate = if council_total > 0 {
        council_converged as f64 / council_total as f64
    } else {
        0.0 // No council events → undefined, report 0
    };

    IntegrationHealthReport {
        total_events,
        plugin_health,
        average_latency_ms,
        error_rate,
        routing_accuracy_estimate,
        council_convergence_rate,
    }
}

/// Format a human-readable summary of the health report.
pub fn format_report(report: &IntegrationHealthReport) -> String {
    let mut lines = vec![
        format!("IntegrationHealthReport (schema={SCHEMA_VERSION})"),
        format!("  total_events: {}", report.total_events),
        format!("  average_latency_ms: {:.1}", report.average_latency_ms),
        format!("  error_rate: {:.1}%", report.error_rate * 100.0),
        format!(
            "  routing_accuracy_estimate: {:.1}%",
            report.routing_accuracy_estimate * 100.0
        ),
        format!(
            "  council_convergence_rate: {:.1}%",
            report.council_convergence_rate * 100.0
        ),
    ];

    if report.plugin_health.is_empty() {
        lines.push("  plugin_health: (no data)".to_string());
    } else {
        lines.push("  plugin_health:".to_string());
        let mut plugins: Vec<_> = report.plugin_health.iter().collect();
        plugins.sort_by_key(|(name, _)| *name);
        for (name, health) in plugins {
            lines.push(format!(
                "    {name}: calls={} errors={} avg_latency_ms={:.1} success_rate={:.1}%",
                health.calls,
                health.errors,
                health.avg_latency_ms,
                health.success_rate * 100.0
            ));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::telemetry::schema::{
        CouncilData, PluginCall, ResultQuality, RoutingOutcome,
    };

    fn make_event(
        outcome: RoutingOutcome,
        plugin_calls: Vec<PluginCall>,
        council: Option<CouncilData>,
    ) -> RoutingDecisionEvent {
        RoutingDecisionEvent {
            schema_version: SCHEMA_VERSION.to_string(),
            timestamp: "2026-04-05T00:00:00Z".to_string(),
            event_type: "routing_decision".to_string(),
            query_fingerprint: "test".to_string(),
            chosen_route: "memory_only".to_string(),
            route_confidence: 0.9,
            plugin_calls,
            end_to_end_latency_ms: 100,
            outcome,
            council_data: council,
        }
    }

    #[test]
    fn empty_events_returns_default_report() {
        let report = aggregate(&[]);
        assert_eq!(report.total_events, 0);
        assert!(report.plugin_health.is_empty());
    }

    #[test]
    fn single_successful_event() {
        let events = vec![make_event(
            RoutingOutcome::Success,
            vec![PluginCall {
                plugin: "memoryport".to_string(),
                latency_ms: 20,
                success: true,
                result_quality: Some(ResultQuality::Useful),
            }],
            None,
        )];

        let report = aggregate(&events);

        assert_eq!(report.total_events, 1);
        assert!((report.average_latency_ms - 100.0).abs() < 0.01);
        assert!((report.error_rate - 0.0).abs() < 0.01);
        assert!((report.routing_accuracy_estimate - 1.0).abs() < 0.01);

        let mh = &report.plugin_health["memoryport"];
        assert_eq!(mh.calls, 1);
        assert_eq!(mh.errors, 0);
        assert!((mh.avg_latency_ms - 20.0).abs() < 0.01);
        assert!((mh.success_rate - 1.0).abs() < 0.01);
    }

    #[test]
    fn mixed_outcomes_affect_error_rate() {
        let events = vec![
            make_event(RoutingOutcome::Success, vec![], None),
            make_event(RoutingOutcome::Partial, vec![], None),
            make_event(RoutingOutcome::Failure, vec![], None),
        ];

        let report = aggregate(&events);

        // 1 partial + 1 failure out of 3 = 2/3 error rate
        assert!((report.error_rate - 2.0 / 3.0).abs() < 0.01);
        // 1 success out of 3 = 1/3 accuracy
        assert!((report.routing_accuracy_estimate - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn multiple_calls_same_plugin() {
        let events = vec![make_event(
            RoutingOutcome::Success,
            vec![
                PluginCall {
                    plugin: "memoryport".to_string(),
                    latency_ms: 10,
                    success: true,
                    result_quality: None,
                },
                PluginCall {
                    plugin: "memoryport".to_string(),
                    latency_ms: 30,
                    success: false,
                    result_quality: None,
                },
            ],
            None,
        )];

        let report = aggregate(&events);

        let mh = &report.plugin_health["memoryport"];
        assert_eq!(mh.calls, 2);
        assert_eq!(mh.errors, 1);
        // avg latency = (10+30)/2 = 20
        assert!((mh.avg_latency_ms - 20.0).abs() < 0.01);
        // success rate = 1/2 = 0.5
        assert!((mh.success_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn council_convergence_rate() {
        let events = vec![
            make_event(
                RoutingOutcome::Success,
                vec![],
                Some(CouncilData {
                    converged: true,
                    rounds: 3,
                    latency_ms: 500,
                }),
            ),
            make_event(
                RoutingOutcome::Success,
                vec![],
                Some(CouncilData {
                    converged: true,
                    rounds: 2,
                    latency_ms: 400,
                }),
            ),
            make_event(
                RoutingOutcome::Failure,
                vec![],
                Some(CouncilData {
                    converged: false,
                    rounds: 5,
                    latency_ms: 800,
                }),
            ),
        ];

        let report = aggregate(&events);
        // 2 converged out of 3 council events = 66.7%
        assert!((report.council_convergence_rate - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn council_convergence_rate_zero_when_no_council() {
        let events = vec![make_event(RoutingOutcome::Success, vec![], None)];

        let report = aggregate(&events);
        assert!((report.council_convergence_rate - 0.0).abs() < 0.01);
    }

    #[test]
    fn format_report_produces_readable_output() {
        let events = vec![make_event(
            RoutingOutcome::Success,
            vec![PluginCall {
                plugin: "memoryport".to_string(),
                latency_ms: 20,
                success: true,
                result_quality: Some(ResultQuality::Useful),
            }],
            None,
        )];

        let report = aggregate(&events);
        let output = format_report(&report);

        assert!(output.contains("total_events: 1"));
        assert!(output.contains("memoryport"));
        assert!(output.contains("calls=1"));
    }
}
