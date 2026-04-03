use anyhow::Result;

use crate::config::{canonical_curated_memory_path, council_files};
use crate::util::{compact, load_jsonl};

pub struct MemoryRecord {
    pub source: String,
    pub timestamp: String,
    pub text: String,
    pub relevance: usize,
}

/// Retrieve memory relevant to `task`, scored by word overlap.
/// Returns up to `limit` records with relevance > 0, sorted descending.
pub fn retrieve_relevant(task: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
    let task_lower = task.to_lowercase();
    let score_fn = |text: &str| -> usize {
        let text_lower = text.to_lowercase();
        task_lower
            .split_whitespace()
            .filter(|w| w.len() > 2 && text_lower.contains(w))
            .count()
    };

    let mut scored = scan_all_stores(score_fn)?;
    scored.sort_by(|a, b| b.relevance.cmp(&a.relevance));
    Ok(scored
        .into_iter()
        .filter(|r| r.relevance > 0)
        .take(limit)
        .collect())
}

/// Retrieve most-recent memory records across all stores, regardless of relevance.
/// Used by council `gather_context`. Returns up to `per_store_limit` records per store.
pub fn retrieve_recent(per_store_limit: usize) -> Result<Vec<MemoryRecord>> {
    let mut out: Vec<MemoryRecord> = Vec::new();

    for (kind, path) in council_files() {
        for record in load_jsonl(&path)?.into_iter().rev().take(per_store_limit) {
            let text = extract_spine_text(&record);
            if !text.is_empty() {
                out.push(MemoryRecord {
                    source: kind.to_string(),
                    timestamp: extract_timestamp(&record),
                    text: compact(text, 200),
                    relevance: 0,
                });
            }
        }
    }

    let curated_path = canonical_curated_memory_path();
    if curated_path.exists() {
        for record in load_jsonl(&curated_path)?
            .into_iter()
            .rev()
            .take(per_store_limit)
        {
            let (entity, text) = extract_curated_text(&record);
            if !text.is_empty() {
                out.push(MemoryRecord {
                    source: format!("curated/{}", entity),
                    timestamp: String::new(),
                    text: compact(text, 200),
                    relevance: 0,
                });
            }
        }
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Scan all memory stores (spine + curated), applying `score_fn` to each record's text.
fn scan_all_stores(score_fn: impl Fn(&str) -> usize) -> Result<Vec<MemoryRecord>> {
    let mut out = Vec::new();

    for (kind, path) in council_files() {
        for record in load_jsonl(&path)? {
            let text = extract_spine_text(&record);
            if text.is_empty() {
                continue;
            }
            out.push(MemoryRecord {
                source: kind.to_string(),
                timestamp: extract_timestamp(&record),
                text: compact(text, 200),
                relevance: score_fn(text),
            });
        }
    }

    let curated_path = canonical_curated_memory_path();
    if curated_path.exists() {
        for record in load_jsonl(&curated_path)? {
            let (entity, text) = extract_curated_text(&record);
            if text.is_empty() {
                continue;
            }
            out.push(MemoryRecord {
                source: format!("curated/{}", entity),
                timestamp: String::new(),
                text: compact(text, 200),
                relevance: score_fn(text),
            });
        }
    }

    Ok(out)
}

/// Extract the best text field from a spine (plans/traces/learnings) JSONL record.
fn extract_spine_text(record: &serde_json::Value) -> &str {
    record
        .get("summary")
        .or_else(|| record.get("task"))
        .or_else(|| record.get("plan_markdown"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

/// Extract (entity, summary_text) from a curated memory record.
fn extract_curated_text(record: &serde_json::Value) -> (&str, &str) {
    let entity = record
        .get("entity")
        .and_then(|v| v.as_str())
        .unwrap_or("record");
    let text = record
        .get("payload")
        .and_then(|p| p.get("summary").or_else(|| p.get("title")))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    (entity, text)
}

fn extract_timestamp(record: &serde_json::Value) -> String {
    record
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string()
}
