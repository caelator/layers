//! Persistent state and artifact types for the technician.

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Schema version for all technician artifacts.
pub const TECHNICIAN_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

use std::path::PathBuf;

fn layers_root() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())).join(".layers")
}

pub fn state_path() -> PathBuf {
    layers_root().join("technician-state.json")
}

pub fn repairs_path() -> PathBuf {
    layers_root().join("technician-repairs.jsonl")
}

pub fn escalations_path() -> PathBuf {
    layers_root().join("technician-escalations.jsonl")
}

#[allow(dead_code)]
pub fn health_path() -> PathBuf {
    layers_root().join(".technician-health.jsonl")
}

#[allow(dead_code)]
pub fn lock_path() -> PathBuf {
    layers_root().join(".technician.lock")
}

// ---------------------------------------------------------------------------
// TechnicianState
// ---------------------------------------------------------------------------

/// Persistent state across technician cycles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicianState {
    /// Schema version for this artifact.
    pub schema_version: u32,
    /// ISO-8601 timestamp of the last completed cycle.
    pub last_cycle_ts: String,
    /// Monotonically increasing cycle counter.
    pub cycle_count: u64,
    /// Whether UC binary + config are available.
    pub uc_available: bool,
    /// Whether `GitNexus` binary is available.
    pub gitnexus_available: bool,
    /// Number of telemetry events seen in the last cycle.
    pub telemetry_event_count: usize,
    /// Error rate from telemetry events (0.0–1.0).
    pub telemetry_error_rate: f64,
    /// Total council runs ever recorded.
    pub council_runs_total: usize,
    /// Council runs with status=failed in the last 7 days.
    pub council_runs_failed_7d: usize,
    /// Number of unresolved escalations.
    pub pending_escalations: usize,
    /// Diagnoses found in the last cycle.
    pub diagnoses_this_cycle: Vec<String>,
    /// Number of repairs applied in the last cycle.
    pub repairs_this_cycle: usize,
    /// Remaining repair budget per category.
    pub repair_budget_remaining: RepairBudget,
    /// Counts per diagnosis type in rolling 24h window (diagnosis → count).
    #[serde(default)]
    pub diagnosis_counts_24h: std::collections::HashMap<String, u32>,
}

impl Default for TechnicianState {
    fn default() -> Self {
        Self {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            last_cycle_ts: Utc::now().to_rfc3339(),
            cycle_count: 0,
            uc_available: false,
            gitnexus_available: false,
            telemetry_event_count: 0,
            telemetry_error_rate: 0.0,
            council_runs_total: 0,
            council_runs_failed_7d: 0,
            pending_escalations: 0,
            diagnoses_this_cycle: Vec::new(),
            repairs_this_cycle: 0,
            repair_budget_remaining: RepairBudget::default(),
            diagnosis_counts_24h: std::collections::HashMap::new(),
        }
    }
}

impl TechnicianState {
    pub fn load() -> Self {
        let path = state_path();
        if !path.exists() {
            return Self::default();
        }
        let Ok(file) = std::fs::File::open(&path) else {
            return Self::default();
        };
        serde_json::from_reader(file).unwrap_or_default()
    }

    pub fn persist(&self) -> std::io::Result<()> {
        let path = state_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        serde_json::to_writer(
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)?,
            self,
        )?;
        Ok(())
    }

    pub fn next_cycle(&mut self) {
        self.cycle_count += 1;
        self.last_cycle_ts = Utc::now().to_rfc3339();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairBudget {
    /// Remaining JSONL truncate operations this cycle.
    pub jsonl_truncate: u32,
    /// Remaining UC config stub creations this cycle.
    pub uc_stub: u32,
    /// Remaining circuit-breaker resets this hour per task-pattern.
    #[serde(default)]
    pub cb_reset: std::collections::HashMap<String, u32>,
}

impl Default for RepairBudget {
    fn default() -> Self {
        Self {
            jsonl_truncate: 3,
            uc_stub: 1,
            cb_reset: std::collections::HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// RepairRecord
// ---------------------------------------------------------------------------

/// A single repair action applied by the technician.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairRecord {
    pub schema_version: u32,
    pub ts: String,
    pub cycle_id: String,
    pub diagnosis: String,
    pub repair_action: String,
    pub path: Option<String>,
    pub lines_removed: Option<usize>,
    pub outcome: RepairOutcome,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairOutcome {
    Applied,
    SkippedNoBudget,
    SkippedDryRun,
    Failed,
}

// ---------------------------------------------------------------------------
// EscalationRecord
// ---------------------------------------------------------------------------

/// A condition that requires human attention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationRecord {
    pub schema_version: u32,
    pub ts: String,
    pub cycle_id: String,
    pub diagnosis: String,
    pub context: serde_json::Value,
    pub repair_attempted: bool,
    pub repair_outcome: String,
    pub escalation_reason: String,
    /// How many times this diagnosis type has occurred in rolling 24h.
    pub diagnosis_count_24h: u32,
}

impl EscalationRecord {
    pub fn new(
        cycle_id: &str,
        diagnosis: &str,
        context: serde_json::Value,
        repair_attempted: bool,
        repair_outcome: &str,
        escalation_reason: &str,
        diagnosis_count_24h: u32,
    ) -> Self {
        Self {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            ts: Utc::now().to_rfc3339(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.to_string(),
            context,
            repair_attempted,
            repair_outcome: repair_outcome.to_string(),
            escalation_reason: escalation_reason.to_string(),
            diagnosis_count_24h,
        }
    }
}

// ---------------------------------------------------------------------------
// CycleReport
// ---------------------------------------------------------------------------

/// The output of a single technician cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleReport {
    pub schema_version: u32,
    pub cycle_id: String,
    pub ts: String,
    pub diagnoses: Vec<Diagnosis>,
    pub repairs: Vec<RepairRecord>,
    pub escalations: Vec<EscalationRecord>,
    pub uc_available: bool,
    pub gitnexus_available: bool,
    pub telemetry_event_count: usize,
    pub telemetry_error_rate: f64,
    pub council_runs_failed_7d: usize,
    pub repair_budget_remaining: RepairBudget,
}

impl Default for CycleReport {
    fn default() -> Self {
        Self {
            schema_version: TECHNICIAN_SCHEMA_VERSION,
            cycle_id: format!("tech-{}", Utc::now().format("%Y%m%dt%H%M")),
            ts: Utc::now().to_rfc3339(),
            diagnoses: Vec::new(),
            repairs: Vec::new(),
            escalations: Vec::new(),
            uc_available: false,
            gitnexus_available: false,
            telemetry_event_count: 0,
            telemetry_error_rate: 0.0,
            council_runs_failed_7d: 0,
            repair_budget_remaining: RepairBudget::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnosis
// ---------------------------------------------------------------------------

/// A detected fault in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    /// The signal family this diagnosis came from.
    pub signal: Signal,
    /// The specific fault type.
    pub kind: DiagnosisKind,
    /// Human-readable description.
    pub summary: String,
    /// Machine-readable context (paths, run IDs, etc.).
    pub context: serde_json::Value,
    /// Whether this can be repaired autonomously.
    pub autonomously_repairable: bool,
    /// Whether this requires escalation.
    pub requires_escalation: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    Plugin,
    Uc,
    GitNexus,
    CouncilArtifacts,
    CircuitBreaker,
    Telemetry,
    RouteCorrections,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosisKind {
    // Plugin
    PluginPanic,
    PluginNotCompiling,
    // UC
    UcBinaryMissing,
    UcConfigMissing,
    UcTimeout,
    UcNonZeroExit,
    // GitNexus
    GitNexusBinaryMissing,
    GitNexusIndexStale,
    GitNexusIndexMissing,
    // Council artifacts
    CouncilRunArtifactsMissing,
    CouncilRunJsonCorrupt,
    CouncilTracesJsonlCorrupt,
    CouncilAuditJsonlCorrupt,
    // Circuit breaker
    CircuitBreakerTripped,
    StageRetriesExhausted,
    StageTimedOut,
    ConvergenceNotReached,
    // Telemetry
    TelemetryFileMissing,
    TelemetryErrorRateHigh,
    TelemetryLatencySpike,
    // Route corrections
    RouteCorrectionsJsonlCorrupt,
}

impl DiagnosisKind {
    pub fn autonomously_repairable(&self) -> bool {
        matches!(
            self,
            Self::CouncilTracesJsonlCorrupt
                | Self::CouncilAuditJsonlCorrupt
                | Self::RouteCorrectionsJsonlCorrupt
                | Self::UcConfigMissing
                | Self::CircuitBreakerTripped
        )
    }

    pub fn requires_escalation(&self) -> bool {
        matches!(
            self,
            Self::StageRetriesExhausted
                | Self::StageTimedOut
                | Self::PluginPanic
                | Self::GitNexusIndexStale
                | Self::TelemetryErrorRateHigh
        )
    }

    pub fn signal(&self) -> Signal {
        match self {
            Self::PluginPanic | Self::PluginNotCompiling => Signal::Plugin,
            Self::UcBinaryMissing
            | Self::UcConfigMissing
            | Self::UcTimeout
            | Self::UcNonZeroExit => Signal::Uc,
            Self::GitNexusBinaryMissing | Self::GitNexusIndexStale | Self::GitNexusIndexMissing => {
                Signal::GitNexus
            }
            Self::CouncilRunArtifactsMissing
            | Self::CouncilRunJsonCorrupt
            | Self::CouncilTracesJsonlCorrupt
            | Self::CouncilAuditJsonlCorrupt => Signal::CouncilArtifacts,
            Self::CircuitBreakerTripped
            | Self::StageRetriesExhausted
            | Self::StageTimedOut
            | Self::ConvergenceNotReached => Signal::CircuitBreaker,
            Self::TelemetryFileMissing
            | Self::TelemetryErrorRateHigh
            | Self::TelemetryLatencySpike => Signal::Telemetry,
            Self::RouteCorrectionsJsonlCorrupt => Signal::RouteCorrections,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::PluginPanic => "plugin_panic",
            Self::PluginNotCompiling => "plugin_not_compiling",
            Self::UcBinaryMissing => "uc_binary_missing",
            Self::UcConfigMissing => "uc_config_missing",
            Self::UcTimeout => "uc_timeout",
            Self::UcNonZeroExit => "uc_non_zero_exit",
            Self::GitNexusBinaryMissing => "gitnexus_binary_missing",
            Self::GitNexusIndexStale => "gitnexus_index_stale",
            Self::GitNexusIndexMissing => "gitnexus_index_missing",
            Self::CouncilRunArtifactsMissing => "council_run_artifacts_missing",
            Self::CouncilRunJsonCorrupt => "council_run_json_corrupt",
            Self::CouncilTracesJsonlCorrupt => "council_traces_jsonl_corrupt",
            Self::CouncilAuditJsonlCorrupt => "council_audit_jsonl_corrupt",
            Self::CircuitBreakerTripped => "circuit_breaker_tripped",
            Self::StageRetriesExhausted => "stage_retries_exhausted",
            Self::StageTimedOut => "stage_timed_out",
            Self::ConvergenceNotReached => "convergence_not_reached",
            Self::TelemetryFileMissing => "telemetry_file_missing",
            Self::TelemetryErrorRateHigh => "telemetry_error_rate_high",
            Self::TelemetryLatencySpike => "telemetry_latency_spike",
            Self::RouteCorrectionsJsonlCorrupt => "route_corrections_jsonl_corrupt",
        }
    }
}

impl Diagnosis {
    pub fn new(kind: DiagnosisKind, summary: String, context: serde_json::Value) -> Self {
        let signal = kind.signal();
        let autonomously_repairable = kind.autonomously_repairable();
        let requires_escalation = kind.requires_escalation();
        Self {
            signal,
            kind,
            summary,
            context,
            autonomously_repairable,
            requires_escalation,
        }
    }
}
