//! `layers telemetry` subcommand — integration health reporting.

use crate::config::memoryport_dir;
use crate::plugins::telemetry::TelemetryPlugin;
use clap::Subcommand;

/// Telemetry subcommands — report generation and event inspection.
#[derive(Subcommand)]
pub enum TelemetryCommands {
    /// Print a human-readable health report of all recorded integration events.
    Report,
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
