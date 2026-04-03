use anyhow::Result;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

use crate::config::canonical_curated_memory_path;
use crate::types::{CuratedImportRecord, Decision, ProjectRecord, ProjectRecordPayload};
use crate::util::{append_jsonl, compact, iso_now, load_jsonl};

pub fn handle_curated_import(file: &str) -> Result<()> {
    let path = Path::new(file);
    if !path.exists() {
        anyhow::bail!("file not found: {}", file);
    }
    let (imported, skipped, errors) = import_curated_memory(path)?;
    let ok = errors == 0;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": ok,
            "file": file,
            "canonical_path": canonical_curated_memory_path(),
            "imported": imported,
            "skipped": skipped,
            "parse_errors": errors,
        }))?
    );
    if !ok {
        anyhow::bail!("{} records failed to parse", errors);
    }
    Ok(())
}

fn import_curated_memory(path: &Path) -> Result<(usize, usize, usize)> {
    let raw_lines = fs::read_to_string(path)?;
    let existing = load_jsonl(&canonical_curated_memory_path())?;
    let mut existing_keys: std::collections::BTreeSet<String> = existing
        .iter()
        .filter_map(|r| r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = 0;

    for line in raw_lines.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        let import: CuratedImportRecord = match serde_json::from_value(parsed) {
            Ok(v) => v,
            Err(e) => {
                anyhow::bail!("record parse error: {}", e);
            }
        };
        let record = curated_import_to_record(import)?;
        if !existing_keys.insert(record.id.clone()) {
            skipped += 1;
            continue;
        }
        append_jsonl(
            &canonical_curated_memory_path(),
            &serde_json::to_value(&record)?,
        )?;
        imported += 1;
    }
    Ok((imported, skipped, errors))
}

pub(crate) fn curated_import_to_record(import: CuratedImportRecord) -> Result<ProjectRecord> {
    let entity = match import.kind.as_str() {
        "decision" | "constraint" | "next_step" | "postmortem" => import.kind.as_str(),
        other => anyhow::bail!(
            "unsupported curated import kind: {}. Valid kinds: decision, constraint, next_step, postmortem",
            other
        ),
    };
    let slug = import
        .summary
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
        .collect::<String>()
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase();
    let id = format!("cm_{}_{}", entity, slug);
    let payload = match entity {
        "decision" => ProjectRecordPayload::Decision(Decision {
            slug: slug.clone(),
            title: compact(&import.summary, 96),
            summary: import.summary.clone(),
            rationale: import.rationale,
        }),
        "constraint" => ProjectRecordPayload::Constraint(crate::types::Constraint {
            slug: slug.clone(),
            title: compact(&import.summary, 96),
            summary: import.summary.clone(),
            impact: String::new(),
        }),
        "next_step" => ProjectRecordPayload::NextStep(crate::types::NextStep {
            slug: slug.clone(),
            title: compact(&import.summary, 96),
            summary: import.summary.clone(),
            owner: String::new(),
        }),
        "postmortem" => ProjectRecordPayload::Postmortem(crate::types::Postmortem {
            slug: slug.clone(),
            title: compact(&import.summary, 96),
            summary: import.summary.clone(),
            root_cause: String::new(),
        }),
        _ => unreachable!(),
    };
    Ok(ProjectRecord {
        id,
        entity: entity.to_string(),
        project: import.project,
        task: None,
        created_at: if import.timestamp.is_empty() {
            iso_now()
        } else {
            import.timestamp
        },
        source: "curated-import".to_string(),
        tags: import.tags,
        archived: false,
        metadata: None,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestWorkspace;
    use crate::util::load_jsonl;
    use serde_json::json;
    use std::fs;

    #[test]
    fn curated_import_deduplicates() {
        let ws = TestWorkspace::new("curated-import-dedup");
        let root = ws.root();

        let import_file = root.join("import.jsonl");
        let record = json!({
            "kind": "decision",
            "project": "layers",
            "summary": "Use direct context gathering instead of routing heuristics.",
            "rationale": "Simpler and more predictable.",
            "timestamp": "2026-04-02T00:00:00Z",
            "tags": ["layers"]
        });
        fs::write(
            &import_file,
            format!("{}\n{}\n", record, record),
        )
        .unwrap();

        handle_curated_import(&import_file.to_string_lossy()).unwrap();

        let records =
            load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
        assert_eq!(records.len(), 1, "duplicate should be skipped");
    }

    #[test]
    fn curated_import_rejects_unsupported_kind() {
        let ws = TestWorkspace::new("curated-import-bad");
        let root = ws.root();
        let import_file = root.join("bad-import.jsonl");
        fs::write(
            &import_file,
            json!({"kind": "status", "project": "x", "summary": "y"}).to_string() + "\n",
        )
        .unwrap();
        let result = handle_curated_import(&import_file.to_string_lossy());
        assert!(result.is_err());
    }
}
