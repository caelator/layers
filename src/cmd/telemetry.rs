//! `layers telemetry` subcommand — integration health reporting.

use crate::config::memoryport_dir;
use crate::plugins::telemetry::schema::{CouncilData, PluginCall, ResultQuality, RoutingOutcome};
use crate::plugins::telemetry::{RoutingDecision, TelemetryPlugin};
use clap::Subcommand;

/// Telemetry subcommands — report generation and event inspection.
#[derive(Subcommand)]
pub enum TelemetryCommands {
    /// Print a human-readable health report of all recorded integration events.
    Report,
}

/// Result of a plugin call — whether it was invoked and its outcome.
#[derive(Debug, Clone, Copy)]
pub enum PluginResult {
    /// Not invoked at all.
    NotInvoked,
    /// Invoked and succeeded (produced results).
    Success,
    /// Invoked but returned empty/no results.
    Empty,
    /// Invoked but errored.
    #[allow(dead_code)]
    Failed,
}

impl PluginResult {
    fn is_invoked(self) -> bool {
        !matches!(self, PluginResult::NotInvoked)
    }

    fn to_quality(self) -> Option<ResultQuality> {
        match self {
            PluginResult::NotInvoked => None,
            PluginResult::Success => Some(ResultQuality::Useful),
            PluginResult::Empty => Some(ResultQuality::Neutral),
            PluginResult::Failed => Some(ResultQuality::Useless),
        }
    }
}

/// Parameters for a query routing decision event.
#[derive(Debug, Clone)]
pub struct QueryEventParams {
    pub query_fingerprint: String,
    pub route: String,
    pub confidence: f64,
    pub memory_result: PluginResult,
    pub memory_latency_ms: u64,
    pub gitnexus_result: PluginResult,
    pub gitnexus_latency_ms: u64,
    pub end_to_end_ms: u64,
}

/// Parameters for a council run decision event.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CouncilEventParams {
    pub query_fingerprint: String,
    pub route: String,
    pub confidence: f64,
    pub memory_result: PluginResult,
    pub memory_latency_ms: u64,
    pub gitnexus_result: PluginResult,
    pub gitnexus_latency_ms: u64,
    pub end_to_end_ms: u64,
    pub converged: bool,
    pub rounds: u32,
    pub council_latency_ms: u64,
}

fn build_plugin_calls(
    memory_result: PluginResult,
    memory_latency_ms: u64,
    gitnexus_result: PluginResult,
    gitnexus_latency_ms: u64,
) -> Vec<PluginCall> {
    let mut calls = Vec::new();
    if memory_result.is_invoked() {
        calls.push(PluginCall {
            plugin: "memoryport".to_string(),
            latency_ms: memory_latency_ms,
            success: matches!(memory_result, PluginResult::Success),
            result_quality: memory_result.to_quality(),
        });
    }
    if gitnexus_result.is_invoked() {
        calls.push(PluginCall {
            plugin: "gitnexus".to_string(),
            latency_ms: gitnexus_latency_ms,
            success: matches!(gitnexus_result, PluginResult::Success),
            result_quality: gitnexus_result.to_quality(),
        });
    }
    calls
}

/// Record a query routing decision event to telemetry.
pub fn record_query_event(params: QueryEventParams) {
    let outcome = match (params.memory_result, params.gitnexus_result) {
        (PluginResult::Success, _) | (_, PluginResult::Success) => RoutingOutcome::Success,
        (a, b) if a.is_invoked() || b.is_invoked() => RoutingOutcome::Partial,
        _ => RoutingOutcome::Failure,
    };

    let decision = RoutingDecision {
        query_fingerprint: params.query_fingerprint,
        chosen_route: params.route,
        route_confidence: params.confidence,
        plugin_calls: build_plugin_calls(
            params.memory_result,
            params.memory_latency_ms,
            params.gitnexus_result,
            params.gitnexus_latency_ms,
        ),
        end_to_end_latency_ms: params.end_to_end_ms,
        outcome,
        council_data: None,
    };

    if let Err(e) = (*super::telemetry_plugin()).record_routing_decision(decision) {
        eprintln!("[telemetry] Warning: failed to record query event: {e}");
    }
}
#[allow(dead_code)]
pub fn record_council_event(params: CouncilEventParams) {
    let outcome = if params.converged {
        RoutingOutcome::Success
    } else {
        RoutingOutcome::Partial
    };

    let decision = RoutingDecision {
        query_fingerprint: params.query_fingerprint,
        chosen_route: params.route,
        route_confidence: params.confidence,
        plugin_calls: build_plugin_calls(
            params.memory_result,
            params.memory_latency_ms,
            params.gitnexus_result,
            params.gitnexus_latency_ms,
        ),
        end_to_end_latency_ms: params.end_to_end_ms,
        outcome,
        council_data: Some(CouncilData {
            converged: params.converged,
            rounds: params.rounds,
            latency_ms: params.council_latency_ms,
        }),
    };

    if let Err(e) = (*super::telemetry_plugin()).record_routing_decision(decision) {
        eprintln!("[telemetry] Warning: failed to record council event: {e}");
    }
}

/// Handle `layers telemetry` subcommand.
pub fn handle_telemetry(command: &TelemetryCommands) -> anyhow::Result<()> {
    match command {
        TelemetryCommands::Report => {
            let plugin = TelemetryPlugin::new(&memoryport_dir());
            println!("{}", plugin.health_report_string());
            Ok(())
        }
    }
}
