//! Repair engine for the technician.
//!
//! Currently a placeholder — repair actions will be implemented in Phase 2.
//! The detection layer is fully functional in Phase 1.

use super::data::{Diagnosis, DiagnosisKind, RepairBudget, RepairOutcome, RepairRecord};

/// Returns true if the given diagnosis has an autonomous repair available.
pub fn can_repair(kind: &DiagnosisKind) -> bool {
    kind.autonomously_repairable()
}

/// Attempt to repair a diagnosis within the given budget.
/// In Phase 1 (dry-run), this always returns `SkippedDryRun`.
pub fn attempt_repair(
    diagnosis: &Diagnosis,
    _budget: &mut RepairBudget,
    dry_run: bool,
    cycle_id: &str,
) -> Option<RepairRecord> {
    if !can_repair(&diagnosis.kind) {
        return None;
    }

    if dry_run {
        return Some(RepairRecord {
            schema_version: super::data::TECHNICIAN_SCHEMA_VERSION,
            ts: crate::util::iso_now(),
            cycle_id: cycle_id.to_string(),
            diagnosis: diagnosis.kind.name().to_string(),
            repair_action: format!("would_repair_{}", diagnosis.kind.name()),
            path: None,
            lines_removed: None,
            outcome: RepairOutcome::SkippedDryRun,
        });
    }

    // Phase 2: actual repair actions
    // For now, record that we would repair
    Some(RepairRecord {
        schema_version: super::data::TECHNICIAN_SCHEMA_VERSION,
        ts: crate::util::iso_now(),
        cycle_id: cycle_id.to_string(),
        diagnosis: diagnosis.kind.name().to_string(),
        repair_action: format!("repair_{}", diagnosis.kind.name()),
        path: None,
        lines_removed: None,
        outcome: RepairOutcome::SkippedDryRun,
    })
}
