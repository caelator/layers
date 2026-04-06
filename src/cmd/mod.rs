pub mod council;
pub mod curated;
pub mod feedback;
pub mod infrastructure;
pub mod monitor;
pub mod query;
pub mod refresh;
pub mod remember;
pub mod technician;
pub mod telemetry;
pub mod validate;

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
