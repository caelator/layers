//! Escalation policy engine.
//!
//! Evaluates diagnoses against repair history and decides when to escalate.

use std::collections::HashMap;
use std::io::Write;

use chrono::{Duration, Utc};

use super::data::{CycleReport, DiagnosisKind, EscalationRecord, RepairRecord};

// ---------------------------------------------------------------------------
// EscalationRecord persistence
// ---------------------------------------------------------------------------

/// Append an escalation record to the escalations log.
pub fn append_escalation(record: &EscalationRecord) -> std::io::Result<()> {
    let path = super::data::escalations_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(record)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rolling 24h diagnosis counter
// ---------------------------------------------------------------------------

/// Load recent escalation records from the last 24 hours and count diagnoses.
pub fn load_recent_diagnosis_counts() -> HashMap<String, u32> {
    let path = super::data::escalations_path();
    if !path.exists() {
        return HashMap::new();
    }

    let cutoff = Utc::now() - Duration::hours(24);
    let mut counts: HashMap<String, u32> = HashMap::new();

    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<EscalationRecord>(line) {
                if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&record.ts) {
                    if ts.with_timezone(&Utc) >= cutoff {
                        *counts.entry(record.diagnosis).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    counts
}

// ---------------------------------------------------------------------------
// Escalation decision
// ---------------------------------------------------------------------------

/// Decide whether each diagnosis requires escalation based on policy rules.
pub fn evaluate_escalations(
    report: &CycleReport,
    diagnosis_counts_24h: &HashMap<String, u32>,
) -> Vec<EscalationRecord> {
    let mut escalations = Vec::new();

    for diagnosis in &report.diagnoses {
        let diagnosis_name = diagnosis.kind.name();
        let count_24h = diagnosis_counts_24h
            .get(diagnosis_name)
            .copied()
            .unwrap_or(0);

        let reason = if matches!(
            diagnosis.kind,
            DiagnosisKind::StageRetriesExhausted | DiagnosisKind::StageTimedOut
        ) {
            Some("human_required: council stage exhausted all retries".to_string())
        } else if count_24h >= 3 {
            Some(format!(
                "repeated_failure: same diagnosis {count_24h} times in rolling 24h"
            ))
        } else if diagnosis.requires_escalation {
            Some(format!("requires_escalation: {diagnosis_name}"))
        } else {
            None
        };

        if let Some(escalation_reason) = reason {
            escalations.push(EscalationRecord::new(
                &report.cycle_id,
                diagnosis_name,
                diagnosis.context.clone(),
                diagnosis.autonomously_repairable,
                if diagnosis.autonomously_repairable {
                    "repair_not_attempted"
                } else {
                    "no_autonomous_repair_available"
                },
                &escalation_reason,
                count_24h,
            ));
        }
    }

    escalations
}

// ---------------------------------------------------------------------------
// Repair record persistence
// ---------------------------------------------------------------------------

/// Append a repair record to the repairs log.
pub fn append_repair(record: &RepairRecord) -> std::io::Result<()> {
    let path = super::data::repairs_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(record)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{line}")?;
    Ok(())
}
