//! Layers Technician — self-healing plugin integration monitor.
//!
//! A resident subsystem that runs inside Layers, continuously monitoring the
//! wiring between Layers and every plugin/integration, self-healing what it
//! can, and escalating what it cannot.
//!
//! ## Design principles
//!
//! - **Root cause, not symptom**: the technician identifies actual faults in
//!   integration wiring, not just indicators of distress.
//! - **Never silent**: every cycle produces a report; every repair is recorded.
//! - **Never destructive**: the technician truncates corrupt JSONL at valid
//!   boundaries and creates stub configs — it never rewrites or deletes data.
//! - **Safety before action**: all repairs run in dry-run mode by default until
//!   explicitly enabled with `--apply`.
//!
//! ## Architecture
//!
//! - [`detection::run_all_detections()`] — runs all six signal detectors
//! - [`repair`] — Phase 2 repair actions (JSONL truncate, config stub, etc.)
//! - [`escalation`] — escalation policy engine
//! - [`data`] — state types, schemas, and persistence

pub mod data;
pub mod detection;
pub mod escalation;
pub mod repair;

use std::fmt::Write;

use chrono::Utc;

use data::{CycleReport, Diagnosis, RepairBudget, TECHNICIAN_SCHEMA_VERSION, TechnicianState};
use detection::run_all_detections;
use escalation::{
    append_escalation, append_repair, evaluate_escalations, load_recent_diagnosis_counts,
};
use repair::{attempt_repair, can_repair};

/// Run one full technician cycle.
///
/// This is the main entry point. It:
/// 1. Loads persistent state
/// 2. Runs all detection suites
/// 3. Evaluates which diagnoses require escalation
/// 4. Attempts repairs within budget (dry-run by default)
/// 5. Persists updated state and appends repair/escalation records
/// 6. Returns a `CycleReport` summarizing the cycle
pub fn run_technician_cycle(dry_run: bool) -> anyhow::Result<CycleReport> {
    let cycle_id = format!("tech-{}", Utc::now().format("%Y%m%dt%H%M"));
    let ts = Utc::now().to_rfc3339();

    let mut state = TechnicianState::load();
    state.next_cycle();
    state.diagnoses_this_cycle.clear();
    state.repairs_this_cycle = 0;
    state.repair_budget_remaining = RepairBudget::default();

    // Load recent 24h diagnosis counts for escalation decisions
    let diagnosis_counts_24h = load_recent_diagnosis_counts();

    // Run all detection suites
    let diagnoses = run_all_detections();
    let diagnoses_this_cycle: Vec<String> = diagnoses
        .iter()
        .map(|d| d.kind.name().to_string())
        .collect();

    // Build cycle report skeleton
    let mut report = CycleReport {
        schema_version: TECHNICIAN_SCHEMA_VERSION,
        cycle_id: cycle_id.clone(),
        ts,
        diagnoses: diagnoses.clone(),
        repairs: Vec::new(),
        escalations: Vec::new(),
        uc_available: !diagnoses.iter().any(|d| {
            matches!(
                d.kind,
                data::DiagnosisKind::UcBinaryMissing | data::DiagnosisKind::UcConfigMissing
            )
        }),
        gitnexus_available: !diagnoses
            .iter()
            .any(|d| matches!(d.kind, data::DiagnosisKind::GitNexusBinaryMissing)),
        telemetry_event_count: 0,
        telemetry_error_rate: 0.0,
        council_runs_failed_7d: diagnoses
            .iter()
            .filter(|d| {
                matches!(
                    d.kind,
                    data::DiagnosisKind::CircuitBreakerTripped
                        | data::DiagnosisKind::StageRetriesExhausted
                        | data::DiagnosisKind::StageTimedOut
                )
            })
            .count(),
        repair_budget_remaining: state.repair_budget_remaining.clone(),
    };

    // Evaluate escalations
    let escalations = evaluate_escalations(&report, &diagnosis_counts_24h);
    for escalation in &escalations {
        let _ = append_escalation(escalation);
    }
    report.escalations.clone_from(&escalations);
    state.pending_escalations = escalations.len();

    // Attempt repairs within budget
    let budget = &mut state.repair_budget_remaining;
    for diagnosis in &diagnoses {
        if !can_repair(&diagnosis.kind) {
            continue;
        }
        if let Some(record) = attempt_repair(diagnosis, budget, dry_run, &cycle_id) {
            let _ = append_repair(&record);
            report.repairs.push(record);
            state.repairs_this_cycle += 1;
        }
    }

    state.diagnoses_this_cycle = diagnoses_this_cycle;
    let _ = state.persist();

    Ok(report)
}

/// Format a cycle report as human-readable text.
pub fn format_cycle_report(report: &CycleReport) -> String {
    let mut out = String::new();
    writeln!(&mut out, "=== Technician Cycle {} ===", report.cycle_id).unwrap();
    writeln!(&mut out, "Time: {}", report.ts).unwrap();
    writeln!(&mut out).unwrap();

    // Integration health
    writeln!(&mut out, "Integration Health").unwrap();
    writeln!(
        &mut out,
        "  UC available:      {}",
        bool_icon(report.uc_available)
    )
    .unwrap();
    writeln!(
        &mut out,
        "  GitNexus available: {}",
        bool_icon(report.gitnexus_available)
    )
    .unwrap();
    writeln!(
        &mut out,
        "  Council failures:   {} (7d)",
        report.council_runs_failed_7d
    )
    .unwrap();
    writeln!(&mut out).unwrap();

    // Diagnoses
    if report.diagnoses.is_empty() {
        writeln!(&mut out, "Diagnoses: none").unwrap();
    } else {
        writeln!(&mut out, "Diagnoses: {} found", report.diagnoses.len()).unwrap();
        for d in &report.diagnoses {
            writeln!(
                &mut out,
                "  [{}] {} — {}",
                d.signal_tag(),
                d.kind.name(),
                d.summary
            )
            .unwrap();
        }
        writeln!(&mut out).unwrap();
    }

    // Repairs
    if report.repairs.is_empty() {
        writeln!(&mut out, "Repairs: none attempted").unwrap();
    } else {
        writeln!(&mut out, "Repairs: {} applied", report.repairs.len()).unwrap();
        for r in &report.repairs {
            writeln!(
                &mut out,
                "  [{}] {} — {}",
                r.outcome.tag(),
                r.repair_action,
                r.diagnosis
            )
            .unwrap();
        }
        writeln!(&mut out).unwrap();
    }

    // Escalations
    if report.escalations.is_empty() {
        writeln!(&mut out, "Escalations: none").unwrap();
    } else {
        writeln!(
            &mut out,
            "Escalations: {} ({} pending)",
            report.escalations.len(),
            report.pending_escalations()
        )
        .unwrap();
        for e in &report.escalations {
            writeln!(&mut out, "  [!] {} — {}", e.diagnosis, e.escalation_reason).unwrap();
        }
        writeln!(&mut out).unwrap();
    }

    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

impl Diagnosis {
    /// Tag string for the signal family.
    fn signal_tag(&self) -> &'static str {
        match self.signal {
            data::Signal::Plugin => "plugin",
            data::Signal::Uc => "uc",
            data::Signal::GitNexus => "gitnexus",
            data::Signal::CouncilArtifacts => "council",
            data::Signal::CircuitBreaker => "cb",
            data::Signal::Telemetry => "telemetry",
            data::Signal::RouteCorrections => "route-corr",
        }
    }
}

impl data::RepairOutcome {
    fn tag(self) -> &'static str {
        match self {
            data::RepairOutcome::Applied => "✅",
            data::RepairOutcome::SkippedNoBudget => "⏭",
            data::RepairOutcome::SkippedDryRun => "🔸",
            data::RepairOutcome::Failed => "❌",
        }
    }
}

fn bool_icon(b: bool) -> &'static str {
    if b { "✅" } else { "❌" }
}

impl data::CycleReport {
    fn pending_escalations(&self) -> usize {
        self.escalations.len()
    }
}
