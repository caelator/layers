//! `layers migrate` — migrate legacy project-records into canonical curated memory.

use anyhow::Result;
use serde_json::json;

use crate::config::{canonical_curated_memory_path, memoryport_dir};
use crate::types::ProjectRecord;
use crate::util::{append_jsonl, load_jsonl};

/// Legacy filenames to scan in the memoryport directory.
const LEGACY_FILENAMES: &[&str] = &["project-records.jsonl", "project_records.jsonl"];

/// Handle the `layers migrate` subcommand.
pub fn handle_migrate(dry_run: bool) -> Result<()> {
    let mp_dir = memoryport_dir();
    let canonical = canonical_curated_memory_path();

    // Find legacy files that exist
    let legacy_files: Vec<_> = LEGACY_FILENAMES
        .iter()
        .map(|f| mp_dir.join(f))
        .filter(|p| p.exists())
        .collect();

    if legacy_files.is_empty() {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "message": "no legacy project-records files found",
                "searched": LEGACY_FILENAMES,
                "directory": mp_dir,
            }))?
        );
        return Ok(());
    }

    // Load existing canonical IDs for deduplication
    let existing = load_jsonl(&canonical)?;
    let mut existing_ids: std::collections::BTreeSet<String> = existing
        .iter()
        .filter_map(|r| {
            r.get("id")
                .and_then(serde_json::Value::as_str)
                .map(std::string::ToString::to_string)
        })
        .collect();

    let mut migrated = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;

    for legacy_path in &legacy_files {
        let records = load_jsonl(legacy_path)?;
        for value in records {
            // Verify it parses as a valid ProjectRecord
            let record: ProjectRecord = if let Ok(r) = serde_json::from_value(value.clone()) {
                r
            } else {
                errors += 1;
                continue;
            };

            if !existing_ids.insert(record.id.clone()) {
                skipped += 1;
                continue;
            }

            if dry_run {
                println!("  would migrate: {} ({})", record.id, record.entity);
            } else {
                append_jsonl(&canonical, &serde_json::to_value(&record)?)?;
            }
            migrated += 1;
        }
    }

    let sources: Vec<String> = legacy_files.iter().map(|p| p.display().to_string()).collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": errors == 0,
            "dry_run": dry_run,
            "sources": sources,
            "canonical_path": canonical,
            "migrated": migrated,
            "skipped": skipped,
            "parse_errors": errors,
        }))?
    );

    if errors > 0 {
        anyhow::bail!("{errors} records failed to parse");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestWorkspace;
    use crate::types::{Decision, ProjectRecordPayload};
    use std::fs;

    fn sample_record(id: &str) -> serde_json::Value {
        serde_json::to_value(ProjectRecord {
            id: id.to_string(),
            entity: "decision".to_string(),
            project: "layers".to_string(),
            task: None,
            created_at: "2026-04-01T00:00:00Z".to_string(),
            source: "council-promote".to_string(),
            tags: vec!["test".to_string()],
            archived: false,
            metadata: None,
            payload: ProjectRecordPayload::Decision(Decision {
                slug: "test-decision".to_string(),
                title: "Test decision".to_string(),
                summary: "A test decision for migration".to_string(),
                rationale: "Testing".to_string(),
            }),
        })
        .unwrap()
    }

    #[test]
    fn dry_run_does_not_write() {
        let ws = TestWorkspace::new("migrate-dry-run");
        let root = ws.root();

        let legacy = root.join("memoryport").join("project-records.jsonl");
        fs::write(&legacy, format!("{}\n", sample_record("rec_1"))).unwrap();

        handle_migrate(true).unwrap();

        let canonical = root.join("memoryport").join("curated-memory.jsonl");
        assert!(
            !canonical.exists() || fs::read_to_string(&canonical).unwrap().trim().is_empty(),
            "dry_run should not write to canonical file"
        );
    }

    #[test]
    fn duplicates_are_skipped() {
        let ws = TestWorkspace::new("migrate-dedup");
        let root = ws.root();

        let legacy = root.join("memoryport").join("project-records.jsonl");
        let rec = sample_record("rec_dup");
        fs::write(&legacy, format!("{rec}\n{rec}\n")).unwrap();

        handle_migrate(false).unwrap();

        let canonical = load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
        assert_eq!(canonical.len(), 1, "duplicate should be skipped");
    }

    #[test]
    fn records_are_correctly_transformed() {
        let ws = TestWorkspace::new("migrate-transform");
        let root = ws.root();

        let legacy = root.join("memoryport").join("project-records.jsonl");
        let rec = sample_record("rec_xform");
        fs::write(&legacy, format!("{rec}\n")).unwrap();

        handle_migrate(false).unwrap();

        let canonical = load_jsonl(&root.join("memoryport").join("curated-memory.jsonl")).unwrap();
        assert_eq!(canonical.len(), 1);
        let migrated = &canonical[0];
        assert_eq!(migrated["id"], "rec_xform");
        assert_eq!(migrated["entity"], "decision");
        assert_eq!(migrated["project"], "layers");
        assert_eq!(migrated["source"], "council-promote");
        assert_eq!(migrated["payload"]["type"], "decision");
        assert_eq!(migrated["payload"]["title"], "Test decision");
    }

    #[test]
    fn no_legacy_files_reports_cleanly() {
        let _ws = TestWorkspace::new("migrate-no-legacy");
        // No legacy files written — should succeed with "no files found"
        handle_migrate(false).unwrap();
    }
}
