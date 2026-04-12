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
//! - **Safety before action**: all repairs run within a per-cycle budget that
//!   limits the blast radius of autonomous repair.
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
pub mod learning;
pub mod repair;

use std::fmt::Write;

use chrono::Utc;

use data::{
    CycleReport, Diagnosis, HealingRecord, RepairBudget, RepairOutcome, TECHNICIAN_SCHEMA_VERSION,
    TechnicianState,
};
use detection::run_all_detections;
use escalation::{
    append_escalation, append_healing, append_repair, evaluate_escalations,
    load_recent_diagnosis_counts,
};
use learning::memoryport::{
    enrich_recurring_failures, merge_failure_memory, query_repair_durability,
    scan_local_healing_history,
};
use repair::{attempt_repair, can_repair, verify_repair};

/// Run one full technician cycle.
///
/// This is the main entry point. It:
/// 1. Loads persistent state
/// 2. Runs all detection suites
/// 3. Evaluates which diagnoses require escalation
/// 4. Attempts repairs within budget (only if `apply` is true)
/// 5. Persists updated state and appends repair/escalation records
/// 6. Returns a `CycleReport` summarizing the cycle
///
/// When `apply` is false, repairable diagnoses produce `SkippedDryRun`
/// records so operators can preview what would change.
pub fn run_technician_cycle(apply: bool) -> anyhow::Result<CycleReport> {
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

    // Phase 3.3: Query MemoryPort for past resolutions on recurring failures.
    // Only queries for diagnoses that have appeared before in the 24h window.
    let diagnosis_kinds: Vec<_> = diagnoses.iter().map(|d| d.kind.clone()).collect();
    let failure_memories = if report_uc_available(&diagnoses) {
        let mut memories = enrich_recurring_failures(&diagnosis_counts_24h, &diagnosis_kinds);
        // Merge with local healing history for richer context
        for (class, memory) in &mut memories {
            let local = scan_local_healing_history(class);
            if !local.is_empty() {
                *memory = merge_failure_memory(memory.clone(), local);
            }
        }
        memories
    } else {
        std::collections::HashMap::new()
    };

    // Build cycle report skeleton
    let mut report = CycleReport {
        schema_version: TECHNICIAN_SCHEMA_VERSION,
        cycle_id: cycle_id.clone(),
        ts,
        diagnoses: diagnoses.clone(),
        repairs: Vec::new(),
        healings: Vec::new(),
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

    // Evaluate escalations and enrich with MemoryPort failure memory
    let escalations = evaluate_escalations(&report, &diagnosis_counts_24h);
    let escalations: Vec<_> = escalations
        .into_iter()
        .map(|esc| {
            if let Some(memory) = failure_memories.get(&esc.diagnosis) {
                esc.with_failure_memory(memory)
            } else {
                esc
            }
        })
        .collect();
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
        if let Some(record) = attempt_repair(diagnosis, budget, &cycle_id, apply) {
            let _ = append_repair(&record);

            // For applied repairs, run verification and emit a HealingRecord
            if matches!(record.outcome, RepairOutcome::Applied) {
                let (verified, mut verify_note) = verify_repair(diagnosis);

                // Phase 3.3: Check repair durability from MemoryPort
                let diagnosis_name = diagnosis.kind.name();
                if let Some(durability) =
                    query_repair_durability(&record.repair_action, diagnosis_name)
                {
                    write!(verify_note, " | {}", durability.summary()).unwrap();
                }

                // Phase 3.3: Build diagnosis_context with failure memory
                let diagnosis_context = failure_memories
                    .get(diagnosis_name)
                    .map(FailureMemory::to_context_value);

                let healing = HealingRecord {
                    schema_version: TECHNICIAN_SCHEMA_VERSION,
                    ts: crate::util::iso_now(),
                    cycle_id: cycle_id.clone(),
                    diagnosis: record.diagnosis.clone(),
                    repair_action: record.repair_action.clone(),
                    path: record.path.clone(),
                    outcome: record.outcome,
                    verified,
                    verify_note,
                    diagnosis_context,
                };
                let _ = append_healing(&healing);
                report.healings.push(healing);
            }

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

    // Healings
    if report.healings.is_empty() {
        writeln!(&mut out, "Healings: none").unwrap();
    } else {
        let verified_count = report.healings.iter().filter(|h| h.verified).count();
        writeln!(
            &mut out,
            "Healings: {} ({} verified)",
            report.healings.len(),
            verified_count
        )
        .unwrap();
        for h in &report.healings {
            let icon = if h.verified { "✅" } else { "⚠️" };
            writeln!(
                &mut out,
                "  [{icon}] {} — {}",
                h.repair_action, h.verify_note
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
            if let Some(memory) = &e.failure_memory {
                writeln!(&mut out, "      Memory: {memory}").unwrap();
            }
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
            data::Signal::Sentry => "sentry",
        }
    }
}

impl data::RepairOutcome {
    fn tag(self) -> &'static str {
        match self {
            data::RepairOutcome::Applied => "✅",
            data::RepairOutcome::SkippedDryRun => "👁",
            data::RepairOutcome::SkippedNoBudget => "⏭",
            data::RepairOutcome::Failed => "❌",
        }
    }
}

fn bool_icon(b: bool) -> &'static str {
    if b { "✅" } else { "❌" }
}

/// Check if UC is available based on diagnoses (no UcBinaryMissing/UcConfigMissing).
fn report_uc_available(diagnoses: &[Diagnosis]) -> bool {
    !diagnoses.iter().any(|d| {
        matches!(
            d.kind,
            data::DiagnosisKind::UcBinaryMissing | data::DiagnosisKind::UcConfigMissing
        )
    })
}

impl data::CycleReport {
    fn pending_escalations(&self) -> usize {
        self.escalations.len()
    }
}
