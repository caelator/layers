use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{canonical_curated_memory_path, council_files};
use crate::util::{compact, load_jsonl};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecordKind {
    Decision,
    Constraint,
    NextStep,
    Postmortem,
    Plan,
    Trace,
    Learning,
    Unknown,
}

impl MemoryRecordKind {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Constraint => "constraint",
            Self::NextStep => "next_step",
            Self::Postmortem => "postmortem",
            Self::Plan => "plan",
            Self::Trace => "trace",
            Self::Learning => "learning",
            Self::Unknown => "unknown",
        }
    }

    fn from_entity(entity: &str) -> Self {
        match entity {
            "decision" => Self::Decision,
            "constraint" => Self::Constraint,
            "next_step" => Self::NextStep,
            "postmortem" => Self::Postmortem,
            "plan" => Self::Plan,
            "trace" => Self::Trace,
            "learning" => Self::Learning,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecordStatus {
    Active,
    Archived,
    Legacy,
}

impl MemoryRecordStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecordConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

impl MemoryRecordConfidence {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMemoryRecord {
    pub id: String,
    pub kind: MemoryRecordKind,
    pub status: MemoryRecordStatus,
    pub confidence: MemoryRecordConfidence,
    pub source: String,
    pub project: String,
    pub created_at: String,
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub relevance: usize,
}

impl CanonicalMemoryRecord {
    #[must_use]
    pub fn source_uri(&self) -> String {
        format!("{}#{}", self.source, self.id)
    }
}

pub struct MemoryRecord {
    pub source: String,
    pub timestamp: String,
    pub text: String,
}

/// List canonical curated memory plus optional legacy council adapters.
pub fn list_canonical(limit: usize, include_legacy: bool) -> Result<Vec<CanonicalMemoryRecord>> {
    let mut records = load_canonical_curated()?;
    if include_legacy {
        records.extend(load_legacy_council()?);
    }
    records.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    records.truncate(limit);
    Ok(records)
}

/// Search canonical curated memory plus optional legacy council adapters.
pub fn search_canonical(
    query: &str,
    limit: usize,
    include_legacy: bool,
) -> Result<Vec<CanonicalMemoryRecord>> {
    let query_lower = query.to_lowercase();
    let score_fn = |text: &str| -> usize {
        let text_lower = text.to_lowercase();
        query_lower
            .split_whitespace()
            .filter(|w| w.len() > 2 && text_lower.contains(w))
            .count()
    };
    let mut records = list_canonical(usize::MAX, include_legacy)?
        .into_iter()
        .map(|mut record| {
            record.relevance = score_fn(&record.text);
            record
        })
        .filter(|record| record.relevance > 0)
        .collect::<Vec<_>>();
    records.sort_by(|a, b| {
        b.relevance
            .cmp(&a.relevance)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    records.truncate(limit);
    Ok(records)
}

/// Show one canonical memory record by id.
pub fn show_canonical(id: &str, include_legacy: bool) -> Result<Option<CanonicalMemoryRecord>> {
    Ok(list_canonical(usize::MAX, include_legacy)?
        .into_iter()
        .find(|record| record.id == id))
}

/// Return inspectable counts for the canonical memory API and legacy adapters.
pub fn audit_canonical() -> Result<Value> {
    let canonical = load_canonical_curated()?;
    let legacy = load_legacy_council()?;
    Ok(serde_json::json!({
        "canonical_path": canonical_curated_memory_path(),
        "canonical_records": canonical.len(),
        "legacy_adapter_records": legacy.len(),
        "active_records": canonical.iter().filter(|record| matches!(record.status, MemoryRecordStatus::Active)).count(),
        "archived_records": canonical.iter().filter(|record| matches!(record.status, MemoryRecordStatus::Archived)).count(),
        "statuses": status_counts(canonical.iter().chain(legacy.iter())),
        "confidences": confidence_counts(canonical.iter().chain(legacy.iter())),
        "kinds": kind_counts(canonical.iter().chain(legacy.iter())),
    }))
}

/// Retrieve memory relevant to `task`, scored by word overlap.
/// Returns up to `limit` records with relevance > 0, sorted descending.
pub fn retrieve_relevant(task: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
    let records = search_canonical(task, limit, true)?;
    Ok(records
        .into_iter()
        .map(|record| MemoryRecord {
            source: record.source_uri(),
            timestamp: record.created_at,
            text: compact(&record.text, 200),
        })
        .collect())
}

/// Retrieve most-recent memory records across all stores, regardless of relevance.
/// Used by council `gather_context`. Returns up to `per_store_limit` records per store.
pub fn retrieve_recent(per_store_limit: usize) -> Result<Vec<MemoryRecord>> {
    Ok(list_canonical(per_store_limit * 4, true)?
        .into_iter()
        .map(|record| MemoryRecord {
            source: record.source_uri(),
            timestamp: record.created_at,
            text: compact(&record.text, 200),
        })
        .collect())
}

fn load_canonical_curated() -> Result<Vec<CanonicalMemoryRecord>> {
    load_jsonl(&canonical_curated_memory_path())?
        .into_iter()
        .filter_map(|record| canonical_from_curated(&record))
        .collect::<Vec<_>>()
        .pipe(Ok)
}

fn load_legacy_council() -> Result<Vec<CanonicalMemoryRecord>> {
    let mut out = Vec::new();
    for (kind, path) in council_files() {
        for (index, record) in load_jsonl(&path)?.into_iter().enumerate() {
            let text = extract_spine_text(&record);
            if text.is_empty() {
                continue;
            }
            out.push(CanonicalMemoryRecord {
                id: format!("legacy-{kind}-{}", index + 1),
                kind: MemoryRecordKind::from_entity(kind),
                status: MemoryRecordStatus::Legacy,
                confidence: MemoryRecordConfidence::Low,
                source: path.to_string_lossy().to_string(),
                project: String::new(),
                created_at: extract_timestamp(&record),
                text: text.to_string(),
                tags: Vec::new(),
                relevance: 0,
            });
        }
    }
    Ok(out)
}

fn canonical_from_curated(record: &Value) -> Option<CanonicalMemoryRecord> {
    let id = record.get("id")?.as_str()?.to_string();
    let entity = record
        .get("entity")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let archived = record
        .get("archived")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tags = record
        .get("tags")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some(CanonicalMemoryRecord {
        id,
        kind: MemoryRecordKind::from_entity(entity),
        status: if archived {
            MemoryRecordStatus::Archived
        } else {
            MemoryRecordStatus::Active
        },
        confidence: confidence_from_record(record),
        source: "memoryport/curated-memory.jsonl".to_string(),
        project: record
            .get("project")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        created_at: record
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        text: extract_curated_text(record).1.to_string(),
        tags,
        relevance: 0,
    })
}

fn confidence_from_record(record: &Value) -> MemoryRecordConfidence {
    record
        .get("metadata")
        .and_then(|metadata| metadata.get("confidence"))
        .and_then(Value::as_str)
        .map_or(
            MemoryRecordConfidence::Medium,
            |confidence| match confidence {
                "high" => MemoryRecordConfidence::High,
                "medium" => MemoryRecordConfidence::Medium,
                "low" => MemoryRecordConfidence::Low,
                _ => MemoryRecordConfidence::Unknown,
            },
        )
}

fn counts_by<'a>(
    records: impl Iterator<Item = &'a CanonicalMemoryRecord>,
    key: impl Fn(&CanonicalMemoryRecord) -> &'static str,
) -> Value {
    let mut counts = serde_json::Map::new();
    for record in records {
        let key = key(record).to_string();
        let count = counts.get(&key).and_then(Value::as_u64).unwrap_or(0) + 1;
        counts.insert(key, Value::from(count));
    }
    Value::Object(counts)
}

fn kind_counts<'a>(records: impl Iterator<Item = &'a CanonicalMemoryRecord>) -> Value {
    counts_by(records, |record| record.kind.as_str())
}

fn status_counts<'a>(records: impl Iterator<Item = &'a CanonicalMemoryRecord>) -> Value {
    counts_by(records, |record| record.status.as_str())
}

fn confidence_counts<'a>(records: impl Iterator<Item = &'a CanonicalMemoryRecord>) -> Value {
    counts_by(records, |record| record.confidence.as_str())
}

/// Extract the best text field from a spine (plans/traces/learnings) JSONL record.
fn extract_spine_text(record: &Value) -> &str {
    record
        .get("summary")
        .or_else(|| record.get("task"))
        .or_else(|| record.get("plan_markdown"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

/// Extract (entity, `summary_text`) from a curated memory record.
fn extract_curated_text(record: &Value) -> (&str, &str) {
    let entity = record
        .get("entity")
        .and_then(Value::as_str)
        .unwrap_or("record");
    let text = record
        .get("payload")
        .and_then(|p| p.get("summary").or_else(|| p.get("title")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    (entity, text)
}

fn extract_timestamp(record: &Value) -> String {
    record
        .get("timestamp")
        .or_else(|| record.get("created_at"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestWorkspace;
    use crate::util::append_jsonl;
    use serde_json::json;

    #[test]
    fn canonical_memory_lists_searches_and_shows_curated_records() {
        let ws = TestWorkspace::new("canonical-memory-api");
        let path = ws.root().join("memoryport").join("curated-memory.jsonl");
        append_jsonl(&path, &json!({
            "id": "cm_decision_context-spine",
            "entity": "decision",
            "project": "layers",
            "created_at": "2026-04-01T00:00:00Z",
            "source": "test",
            "tags": ["context"],
            "payload": {"type": "decision", "slug": "context-spine", "title": "Context spine", "summary": "ContextPacket is the product artifact."}
        }))
        .unwrap();

        let listed = list_canonical(10, false).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind.as_str(), "decision");
        assert_eq!(listed[0].status.as_str(), "active");
        assert_eq!(listed[0].confidence.as_str(), "medium");

        let searched = search_canonical("ContextPacket artifact", 10, false).unwrap();
        assert_eq!(searched[0].id, "cm_decision_context-spine");
        assert!(searched[0].relevance > 0);

        let shown = show_canonical("cm_decision_context-spine", false)
            .unwrap()
            .unwrap();
        assert_eq!(
            shown.source_uri(),
            "memoryport/curated-memory.jsonl#cm_decision_context-spine"
        );
    }

    #[test]
    fn canonical_memory_reads_legacy_council_files_as_adapter_records() {
        let ws = TestWorkspace::new("canonical-memory-legacy");
        let path = ws.root().join("memoryport").join("council-learnings.jsonl");
        append_jsonl(
            &path,
            &json!({
                "timestamp": "2026-04-02T00:00:00Z",
                "summary": "Legacy learning remains searchable through adapter."
            }),
        )
        .unwrap();

        let records = search_canonical("Legacy adapter", 10, true).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind.as_str(), "learning");
        assert_eq!(records[0].status.as_str(), "legacy");
    }
}
