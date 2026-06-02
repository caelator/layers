pub mod autoresearch;
pub mod chat;
pub mod config_cmd;
pub mod council;
pub mod curated;
pub mod feedback;
pub mod gate;
pub mod impact;
pub mod infrastructure;
pub mod init;
pub mod mcp;
pub mod migrate;
#[cfg(feature = "substrate-storage")]
pub mod monitor;
pub mod packet;
pub mod preflight;
pub mod query;
pub mod refresh;
pub mod remember;
pub mod research;
#[cfg(feature = "substrate-storage")]
pub mod technician;
pub mod telemetry;
pub mod validate;
pub mod workflow_benchmark;

use crate::config::memoryport_dir;
use crate::plugins::telemetry::TelemetryPlugin;
use std::sync::{LazyLock, Mutex};

/// Global telemetry plugin — initialized once on first use.
static TELEMETRY_PLUGIN: LazyLock<Mutex<TelemetryPlugin>> =
    LazyLock::new(|| Mutex::new(TelemetryPlugin::new(&memoryport_dir())));

/// Access the global telemetry plugin for recording events.
pub fn telemetry_plugin() -> std::sync::MutexGuard<'static, TelemetryPlugin> {
    TELEMETRY_PLUGIN
        .lock()
        .expect("telemetry plugin lock poisoned")
}
