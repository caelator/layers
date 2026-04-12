//! `MemoryPort` failure memory — query past resolutions on recurring failures.
//!
//! When the technician detects a recurring failure (same `DiagnosisKind` seen
//! multiple times in 24h), this module queries `MemoryPort`/UC for past
//! resolutions of the same failure class. The results are:
//!
//! 1. Included in `HealingRecord.diagnosis_context` for the repair audit trail
//! 2. Included in `EscalationRecord.failure_memory` for fix agent prompts
//! 3. Used by the verification step to check repair durability
//!
//! If `MemoryPort` is unavailable, the query degrades gracefully — the technician
//! proceeds without historical context and logs the fallback reason.

use std::collections::HashMap;

use crate::uc::{UcOptions, UcRetriever};

use super::super::data::{DiagnosisKind, FailureMemory, HealingRecord, PastResolution};

/// Query `MemoryPort` for past resolutions of a failure class.
///
/// Builds a semantic query like `"failure_class: {kind} recurrence"` and
/// parses the returned lines into `PastResolution` entries.
pub fn query_failure_memory(kind: &DiagnosisKind) -> FailureMemory {
    let failure_class = kind.name().to_string();
    let query = format!("failure_class: {failure_class} recurrence");

    let retriever = UcRetriever::new(UcOptions::default());
    let result = retriever.retrieve(&query, 5);

    if let Some(reason) = &result.fallback_reason {
        return FailureMemory {
            failure_class,
            past_resolutions: Vec::new(),
            query_succeeded: false,
            fallback_reason: Some(reason.clone()),
        };
    }

    let past_resolutions = parse_resolutions(&result.lines);

    FailureMemory {
        failure_class,
        past_resolutions,
        query_succeeded: true,
        fallback_reason: if result.lines.is_empty() {
            Some("no past resolutions found".into())
        } else {
            None
        },
    }
}

/// Query MemoryPort for durability of past repairs of a given class.
///
/// Used during verification to check whether similar repairs in the past
/// regressed, which informs confidence in the current repair.
pub fn query_repair_durability(
    repair_action: &str,
    failure_class: &str,
) -> Option<DurabilityReport> {
    let query = format!("repair_class: {repair_action} durability failure_class: {failure_class}");

    let retriever = UcRetriever::new(UcOptions::default());
    let result = retriever.retrieve(&query, 5);

    if result.fallback_reason.is_some() || result.lines.is_empty() {
        return None;
    }

    let mut total = 0u32;
    let mut durable = 0u32;
    let mut regressed = 0u32;

    for line in &result.lines {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            total += 1;
            match val.get("durable").and_then(|v| v.as_bool()) {
                Some(true) => durable += 1,
                Some(false) => regressed += 1,
                None => {}
            }
        }
    }

    if total == 0 {
        return None;
    }

    Some(DurabilityReport {
        repair_action: repair_action.to_string(),
        failure_class: failure_class.to_string(),
        total_past_repairs: total,
        durable_count: durable,
        regressed_count: regressed,
    })
}

/// Enrich all diagnoses that are recurring (count_24h > 0) with MemoryPort
/// failure memory. Returns a map from diagnosis name to `FailureMemory`.
///
/// Only queries for diagnoses that have appeared before in the rolling 24h
/// window to avoid unnecessary MemoryPort load.
pub fn enrich_recurring_failures(
    diagnosis_counts_24h: &HashMap<String, u32>,
    current_diagnoses: &[DiagnosisKind],
) -> HashMap<String, FailureMemory> {
    let mut memories = HashMap::new();

    for kind in current_diagnoses {
        let name = kind.name();
        let count = diagnosis_counts_24h.get(name).copied().unwrap_or(0);

        // Only query MemoryPort for recurring failures (seen before in 24h)
        if count > 0 {
            let memory = query_failure_memory(kind);
            memories.insert(name.to_string(), memory);
        }
    }

    memories
}

/// Scan local healing records for past repairs of the same failure class
/// to supplement MemoryPort results with local history.
pub fn scan_local_healing_history(failure_class: &str) -> Vec<PastResolution> {
    let path = super::super::data::healing_path();
    if !path.exists() {
        return Vec::new();
    }

    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };

    let mut resolutions = Vec::new();
    for line in content.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<HealingRecord>(line) {
            if record.diagnosis == failure_class {
                resolutions.push(PastResolution {
                    date: record.ts.clone(),
                    repair_action: record.repair_action.clone(),
                    details: record.verify_note.clone(),
                    durable: Some(record.verified),
                });
            }
        }
        // Cap at 10 local records to avoid scanning the entire file
        if resolutions.len() >= 10 {
            break;
        }
    }

    resolutions
}

/// Merge MemoryPort results with local healing history into a single
/// `FailureMemory`, deduplicating by date+action.
pub fn merge_failure_memory(
    mut uc_memory: FailureMemory,
    local_resolutions: Vec<PastResolution>,
) -> FailureMemory {
    // Local resolutions supplement MemoryPort — add any that aren't duplicates
    for local in local_resolutions {
        let already_present = uc_memory
            .past_resolutions
            .iter()
            .any(|r| r.date == local.date && r.repair_action == local.repair_action);
        if !already_present {
            uc_memory.past_resolutions.push(local);
        }
    }

    // Sort most recent first
    uc_memory
        .past_resolutions
        .sort_by(|a, b| b.date.cmp(&a.date));

    uc_memory
}

/// Report on the durability of past repairs for a failure class.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DurabilityReport {
    pub repair_action: String,
    pub failure_class: String,
    pub total_past_repairs: u32,
    pub durable_count: u32,
    pub regressed_count: u32,
}

impl DurabilityReport {
    /// Fraction of past repairs that held (0.0–1.0).
    pub fn durability_rate(&self) -> f64 {
        if self.total_past_repairs == 0 {
            return 0.0;
        }
        f64::from(self.durable_count) / f64::from(self.total_past_repairs)
    }

    /// Human-readable summary for verification notes.
    pub fn summary(&self) -> String {
        format!(
            "Past repairs of '{}' for '{}': {}/{} durable ({:.0}%), {}/{} regressed",
            self.repair_action,
            self.failure_class,
            self.durable_count,
            self.total_past_repairs,
            self.durability_rate() * 100.0,
            self.regressed_count,
            self.total_past_repairs,
        )
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse UC retrieval lines into `PastResolution` entries.
///
/// UC returns lines that may be plain text or JSON. We try JSON first,
/// then fall back to extracting what we can from plain text.
fn parse_resolutions(lines: &[String]) -> Vec<PastResolution> {
    let mut resolutions = Vec::new();

    for line in lines {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            // Structured JSON from curated memory
            let date = val
                .get("date")
                .or_else(|| val.get("ts"))
                .or_else(|| val.get("timestamp"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let repair_action = val
                .get("repair_action")
                .or_else(|| val.get("action"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let details = val
                .get("details")
                .or_else(|| val.get("resolution"))
                .or_else(|| val.get("summary"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let durable = val.get("durable").and_then(|v| v.as_bool());

            resolutions.push(PastResolution {
                date,
                repair_action,
                details,
                durable,
            });
        } else {
            // Plain text — extract what we can
            resolutions.push(PastResolution {
                date: "unknown".to_string(),
                repair_action: "unknown".to_string(),
                details: line.clone(),
                durable: None,
            });
        }
    }

    resolutions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_resolutions_handles_json_lines() {
        let lines = vec![
            r#"{"date":"2026-04-08T12:00:00Z","repair_action":"jsonl_truncate","details":"Truncated 3 corrupt lines","durable":true}"#.to_string(),
            r#"{"ts":"2026-04-07T10:00:00Z","action":"cb_reset","resolution":"Reset circuit breaker for run-42"}"#.to_string(),
        ];

        let resolutions = parse_resolutions(&lines);
        assert_eq!(resolutions.len(), 2);
        assert_eq!(resolutions[0].date, "2026-04-08T12:00:00Z");
        assert_eq!(resolutions[0].repair_action, "jsonl_truncate");
        assert_eq!(resolutions[0].durable, Some(true));
        assert_eq!(resolutions[1].date, "2026-04-07T10:00:00Z");
        assert_eq!(resolutions[1].repair_action, "cb_reset");
        assert!(resolutions[1].durable.is_none());
    }

    #[test]
    fn parse_resolutions_handles_plain_text() {
        let lines = vec!["Manually restarted the UC daemon after config corruption".to_string()];
        let resolutions = parse_resolutions(&lines);
        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].repair_action, "unknown");
        assert_eq!(
            resolutions[0].details,
            "Manually restarted the UC daemon after config corruption"
        );
    }

    #[test]
    fn failure_memory_summary_with_resolutions() {
        let memory = FailureMemory {
            failure_class: "council_traces_jsonl_corrupt".to_string(),
            past_resolutions: vec![
                PastResolution {
                    date: "2026-04-08".to_string(),
                    repair_action: "jsonl_truncate".to_string(),
                    details: "Truncated 3 corrupt lines".to_string(),
                    durable: Some(true),
                },
                PastResolution {
                    date: "2026-04-06".to_string(),
                    repair_action: "jsonl_truncate".to_string(),
                    details: "Truncated 1 corrupt line".to_string(),
                    durable: Some(false),
                },
            ],
            query_succeeded: true,
            fallback_reason: None,
        };

        let summary = memory.summary();
        assert!(summary.contains("2026-04-08"));
        assert!(summary.contains("Truncated 3 corrupt lines"));
        assert!(summary.contains("2 past resolution(s)"));
        assert!(summary.contains("1 durable"));
    }

    #[test]
    fn failure_memory_summary_empty() {
        let memory = FailureMemory {
            failure_class: "uc_timeout".to_string(),
            past_resolutions: vec![],
            query_succeeded: true,
            fallback_reason: Some("no past resolutions found".into()),
        };

        let summary = memory.summary();
        assert!(summary.contains("No past resolutions"));
    }

    #[test]
    fn merge_deduplicates_and_sorts() {
        let uc_memory = FailureMemory {
            failure_class: "test".to_string(),
            past_resolutions: vec![PastResolution {
                date: "2026-04-06".to_string(),
                repair_action: "fix_a".to_string(),
                details: "from uc".to_string(),
                durable: Some(true),
            }],
            query_succeeded: true,
            fallback_reason: None,
        };

        let local = vec![
            PastResolution {
                date: "2026-04-08".to_string(),
                repair_action: "fix_b".to_string(),
                details: "from local".to_string(),
                durable: Some(true),
            },
            // Duplicate of the UC entry
            PastResolution {
                date: "2026-04-06".to_string(),
                repair_action: "fix_a".to_string(),
                details: "from local (dup)".to_string(),
                durable: Some(true),
            },
        ];

        let merged = merge_failure_memory(uc_memory, local);
        assert_eq!(merged.past_resolutions.len(), 2); // no duplicate
        assert_eq!(merged.past_resolutions[0].date, "2026-04-08"); // most recent first
    }

    #[test]
    fn durability_report_rate() {
        let report = DurabilityReport {
            repair_action: "jsonl_truncate".to_string(),
            failure_class: "council_traces_jsonl_corrupt".to_string(),
            total_past_repairs: 4,
            durable_count: 3,
            regressed_count: 1,
        };
        assert!((report.durability_rate() - 0.75).abs() < f64::EPSILON);
        assert!(report.summary().contains("75%"));
    }

    #[test]
    fn durability_report_zero_total() {
        let report = DurabilityReport {
            repair_action: "test".to_string(),
            failure_class: "test".to_string(),
            total_past_repairs: 0,
            durable_count: 0,
            regressed_count: 0,
        };
        assert!((report.durability_rate()).abs() < f64::EPSILON);
    }

    #[test]
    fn enrich_only_queries_recurring() {
        // With no counts in 24h, no queries should fire (and no UC needed)
        let counts = HashMap::new();
        let diagnoses = vec![DiagnosisKind::UcTimeout];
        let memories = enrich_recurring_failures(&counts, &diagnoses);
        assert!(memories.is_empty());
    }
}
