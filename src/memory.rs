use anyhow::Result;
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use std::process::Command;

use crate::config::{council_files, uc_config_path, workspace_root};
use crate::projects::{search_curated_records, search_project_records};
use crate::types::{GraphContext, MemoryBrief, MemoryHit};
use crate::util::{compact, load_jsonl, tokenize, which};

fn summarize_record(kind: &str, record: &Value) -> String {
    match kind {
        "plan" => compact(
            &format!(
                "{} {}",
                record.get("task").and_then(|v| v.as_str()).unwrap_or(""),
                record
                    .get("plan_markdown")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
            ),
            260,
        ),
        "trace" => compact(
            record
                .get("task")
                .and_then(|v| v.as_str())
                .or_else(|| record.get("summary").and_then(|v| v.as_str()))
                .unwrap_or(""),
            260,
        ),
        "learning" => compact(
            record
                .get("summary")
                .and_then(|v| v.as_str())
                .or_else(|| record.get("task").and_then(|v| v.as_str()))
                .unwrap_or(""),
            260,
        ),
        _ => compact(&record.to_string(), 260),
    }
}

pub fn search_memory_semantic(
    query: &str,
    limit: usize,
) -> Result<(Vec<MemoryHit>, Option<String>)> {
    let uc = which("uc");
    let config = uc_config_path();
    if uc.is_none() || !config.exists() {
        return Ok((
            vec![],
            Some("uc or ~/.memoryport/uc.toml unavailable".to_string()),
        ));
    }
    let uc = uc.unwrap();
    let output = Command::new(uc)
        .args([
            "-c",
            config.to_str().unwrap(),
            "retrieve",
            query,
            "--top-k",
            &format!("{}", limit.max(10)),
        ])
        .current_dir(workspace_root())
        .output()?;
    if !output.status.success() {
        return Ok((
            vec![],
            Some(compact(&String::from_utf8_lossy(&output.stderr), 400)),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let blocks: Vec<&str> = stdout
        .split("--- Result ")
        .filter(|s| !s.trim().is_empty())
        .collect();
    let score_re = Regex::new(r"\(score:\s*([-0-9.]+)\)")?;
    let session_re = Regex::new(r"Session:\s*(.+)")?;
    let kind_re = Regex::new(r"Type:\s*(.+)")?;
    let timestamp_re = Regex::new(r"Time:\s*(.+)")?;
    let content_re = Regex::new(r"Content:\s*([\s\S]+)")?;
    let mut hits = Vec::new();
    for block in blocks {
        let score = score_re
            .captures(block)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<f64>().ok());
        let session = session_re
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string());
        let kind = kind_re
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_lowercase())
            .unwrap_or_else(|| "memoryport".to_string());
        let timestamp = timestamp_re
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string());
        let content = content_re
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| compact(m.as_str(), 260))
            .unwrap_or_default();
        if looks_low_signal_memory(&content) {
            continue;
        }
        hits.push(MemoryHit {
            kind,
            score,
            timestamp,
            task: session,
            summary: content,
            artifacts_dir: None,
            source: "memoryport-semantic".to_string(),
            graph_context: None,
        });
    }
    hits.sort_by_key(|h| std::cmp::Reverse(memory_hit_rank(query, h)));
    hits.truncate(limit);
    Ok((hits, None))
}

pub fn search_memory_fallback(query: &str, limit: usize) -> Result<Vec<MemoryHit>> {
    let tokens = tokenize(query);
    let mut ranked: Vec<(i32, MemoryHit)> = Vec::new();
    for (kind, path) in council_files() {
        for record in load_jsonl(&path)? {
            let hay = summarize_record(kind, &record).to_lowercase();
            let hay_tokens = tokenize(&hay);
            let overlap = tokens.intersection(&hay_tokens).count() as i32;
            if overlap <= 0 {
                continue;
            }
            let mut score = overlap;
            if kind == "plan" {
                score += 2;
            }
            if kind == "learning" {
                score += 1;
            }
            ranked.push((
                score,
                MemoryHit {
                    kind: kind.to_string(),
                    score: Some(score as f64),
                    timestamp: record
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    task: record
                        .get("task")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    summary: summarize_record(kind, &record),
                    artifacts_dir: record
                        .get("artifacts_dir")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    source: "jsonl-fallback".to_string(),
                    graph_context: parse_graph_context(&record),
                },
            ));
        }
    }
    ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    Ok(ranked.into_iter().take(limit).map(|(_, hit)| hit).collect())
}

fn parse_graph_context(record: &Value) -> Option<GraphContext> {
    record
        .get("metadata")
        .and_then(|v| v.get("graph_context"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn memory_kind_weight(kind: &str) -> i32 {
    match kind {
        "decision" => 8,
        "constraint" => 7,
        "next_step" => 6,
        "status" => 5,
        "task" => 4,
        "project" => 3,
        "postmortem" => 2,
        "knowledge" => 4,
        "plan" => 3,
        "learning" => 3,
        "trace" => 2,
        "conversation" => 0,
        _ => 1,
    }
}

fn artifact_weight(hit: &MemoryHit) -> i32 {
    let artifacts_dir = hit.artifacts_dir.clone().unwrap_or_default().to_lowercase();
    let summary = hit.summary.to_lowercase();
    let task = hit.task.clone().unwrap_or_default().to_lowercase();
    let mut weight = 0;
    if artifacts_dir.contains("boss-pass")
        || summary.contains("decision log")
        || task.contains("decision log")
    {
        weight += 4;
    }
    if summary.contains("final") || task.contains("final") {
        weight += 2;
    }
    if artifacts_dir.contains("round2") {
        weight += 1;
    }
    weight
}

fn memory_hit_rank(query: &str, hit: &MemoryHit) -> (i32, i32, i32, usize) {
    let overlap = tokenize(query)
        .intersection(&tokenize(&hit.summary))
        .count() as i32;
    let numeric = hit.score.unwrap_or(-999.0);
    let kind_bonus = memory_kind_weight(&hit.kind);
    let artifact_bonus = artifact_weight(hit);
    (
        overlap + kind_bonus + artifact_bonus + memory_source_weight(&hit.source),
        artifact_bonus,
        (numeric * 1000.0) as i32,
        hit.summary.len(),
    )
}

fn memory_source_weight(source: &str) -> i32 {
    match source {
        "curated-memory" => 6,
        "structured-records" => 3,
        "memoryport-semantic" => 2,
        "jsonl-fallback" => 1,
        _ => 0,
    }
}

fn looks_low_signal_memory(summary: &str) -> bool {
    let lowered = summary.trim().to_lowercase();
    if lowered.is_empty() || lowered.len() < 12 {
        return true;
    }
    let low = ["gh auth login", "ok", "done", "thanks", "yes", "no", "test"];
    if low.contains(&lowered.as_str()) {
        return true;
    }
    Regex::new(r"^[a-z0-9_./:-]+$").unwrap().is_match(&lowered)
        && lowered.split_whitespace().count() <= 3
}

fn curated_memory_label(hit: &MemoryHit) -> Option<&'static str> {
    match hit.kind.to_lowercase().as_str() {
        "decision" => Some("Decision"),
        "constraint" => Some("Constraint"),
        "status" => Some("Status"),
        "next_step" => Some("Next step"),
        "postmortem" => Some("Postmortem"),
        _ => None,
    }
}

fn curated_memory_text(hit: &MemoryHit) -> String {
    compact(&distill_summary(hit), 220)
}

fn brief_push(bucket: &mut Vec<String>, seen: &mut BTreeSet<String>, value: String, limit: usize) {
    if bucket.len() >= limit {
        return;
    }
    if seen.insert(value.clone()) {
        bucket.push(value);
    }
}

pub fn search_memory(query: &str, limit: usize) -> Result<(Vec<MemoryHit>, Option<String>)> {
    let curated_hits = search_curated_records(query, limit)?;
    let structured_hits = search_project_records(query, limit)?;
    let (semantic_hits, semantic_issue) = search_memory_semantic(query, limit)?;
    let fallback_hits = search_memory_fallback(query, limit)?;
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    let groups = [curated_hits, structured_hits, semantic_hits, fallback_hits];
    for mut group in groups {
        group.sort_by_key(|h| std::cmp::Reverse(memory_hit_rank(query, h)));
        for hit in group {
            let key = format!("{}::{}", hit.kind, hit.summary);
            if seen.insert(key) {
                deduped.push(hit);
            }
            if deduped.len() >= limit {
                return Ok((deduped, semantic_issue));
            }
        }
    }
    if !deduped.is_empty() {
        return Ok((deduped, semantic_issue));
    }
    Ok((vec![], semantic_issue))
}

fn distill_summary(hit: &MemoryHit) -> String {
    let summary = compact(&hit.summary, 220);
    let kind = hit.kind.to_lowercase();
    let task = compact(hit.task.as_deref().unwrap_or(""), 120);
    if kind == "plan" && !task.is_empty() {
        let task_lower = task.to_lowercase();
        if task_lower.contains("critique") || task_lower.contains("revise") {
            return compact(&format!("Plan revision artifact: {}", task), 220);
        }
        if task_lower.contains("continue the layers planning process") {
            return "Later Layers planning round focused on improving routing, critique handling, and implementation readiness.".to_string();
        }
        if task_lower.contains("plugin/integration layer called") && task_lower.contains("layers") {
            return "Initial Layers architecture plan defining Memoryport + GitNexus integration and routing goals.".to_string();
        }
        return compact(&format!("Plan artifact: {}", task), 220);
    }
    if kind == "knowledge" {
        return summary;
    }
    if kind == "learning" {
        return compact(&format!("Learning: {}", summary), 220);
    }
    summary
}

pub fn synthesize_memory_brief(memory_hits: &[MemoryHit]) -> MemoryBrief {
    let mut brief = MemoryBrief::default();
    let mut seen = BTreeSet::new();
    for hit in memory_hits {
        if let Some(label) = curated_memory_label(hit) {
            let item = format!("{}: {}", label, curated_memory_text(hit));
            match hit.kind.to_lowercase().as_str() {
                "decision" => brief_push(&mut brief.decisions, &mut seen, item, 2),
                "constraint" => brief_push(&mut brief.constraints, &mut seen, item, 2),
                "status" => brief_push(&mut brief.status, &mut seen, item, 2),
                "next_step" => brief_push(&mut brief.next_steps, &mut seen, item, 2),
                "postmortem" => brief_push(&mut brief.postmortems, &mut seen, item, 1),
                _ => {}
            }
            continue;
        }
        let item = match hit.kind.to_lowercase().as_str() {
            "learning" => format!("Learning: {}", curated_memory_text(hit)),
            "plan" => format!("Plan: {}", curated_memory_text(hit)),
            "trace" => format!("Trace: {}", curated_memory_text(hit)),
            "project" => format!("Project: {}", curated_memory_text(hit)),
            "task" => format!("Task: {}", curated_memory_text(hit)),
            _ => distill_summary(hit),
        };
        brief_push(&mut brief.notable_context, &mut seen, item, 3);
    }
    if brief.status.is_empty() && !memory_hits.is_empty() {
        brief.status.push(
            "Status: relevant memory artifacts were retrieved, but no canonical status record matched the query.".to_string(),
        );
    }
    if brief.next_steps.is_empty() && !memory_hits.is_empty() {
        brief.next_steps.push(
            "Next step: promote the current implementation direction into an explicit next_step record once it stabilizes.".to_string(),
        );
    }
    brief
}

pub fn format_memory_hit(hit: &MemoryHit) -> String {
    let mut parts = vec![
        hit.kind.clone(),
        hit.timestamp
            .clone()
            .unwrap_or_else(|| "unknown-time".to_string()),
    ];
    if hit.source == "memoryport-semantic"
        && let Some(score) = hit.score
    {
        parts.push(format!("score={:.4}", score));
    }
    if hit.source == "curated-memory" {
        parts.push("preferred".to_string());
    }
    if hit.graph_context.is_some() {
        parts.push("graph-context".to_string());
    }
    let mut base = format!("{} | {}", parts.join(" | "), distill_summary(hit));
    if let Some(label) = curated_memory_label(hit) {
        base.push_str(&format!(" || {} anchor", label));
    }
    base
}

#[cfg(test)]
mod tests {
    use super::{search_memory, synthesize_memory_brief};
    use crate::test_support::workspace_lock;
    use crate::types::{Decision, MemoryHit, ProjectRecord, ProjectRecordPayload, Task};
    use crate::util::append_jsonl;
    use std::fs;
    use std::path::PathBuf;

    fn temp_workspace(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "layers-memory-tests-{}-{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("memoryport")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        dir
    }

    #[test]
    fn typed_hits_drive_memory_brief() {
        let hits = vec![
            MemoryHit {
                kind: "decision".to_string(),
                score: Some(9.0),
                timestamp: None,
                task: Some("layers".to_string()),
                summary: "Use Memoryport for continuity and GitNexus for structure.".to_string(),
                artifacts_dir: None,
                source: "curated-memory".to_string(),
                graph_context: None,
            },
            MemoryHit {
                kind: "constraint".to_string(),
                score: Some(8.0),
                timestamp: None,
                task: Some("layers".to_string()),
                summary: "Keep providers local-first and explicit.".to_string(),
                artifacts_dir: None,
                source: "project-records".to_string(),
                graph_context: None,
            },
            MemoryHit {
                kind: "learning".to_string(),
                score: Some(5.0),
                timestamp: None,
                task: Some("layers".to_string()),
                summary: "Past planning drifted when architecture summaries were too loose."
                    .to_string(),
                artifacts_dir: None,
                source: "memoryport-semantic".to_string(),
                graph_context: None,
            },
        ];

        let brief = synthesize_memory_brief(&hits);
        assert_eq!(brief.decisions.len(), 1);
        assert_eq!(brief.constraints.len(), 1);
        assert!(brief.status[0].starts_with("Status:"));
        assert!(
            brief
                .notable_context
                .iter()
                .any(|item| item.starts_with("Learning:"))
        );
    }

    #[test]
    fn search_memory_prefers_curated_hits() {
        let _guard = workspace_lock().lock().unwrap();
        let original = std::env::var_os("LAYERS_WORKSPACE_ROOT");
        let root = temp_workspace("preferred-order");
        unsafe {
            std::env::set_var("LAYERS_WORKSPACE_ROOT", &root);
        }

        let curated_path = root.join("memoryport").join("curated-memory.jsonl");
        let fallback_path = root.join("memoryport").join("council-traces.jsonl");

        let decision = ProjectRecord {
            id: "cm_decision_layers_curated-memory-first".to_string(),
            entity: "decision".to_string(),
            project: "layers".to_string(),
            task: None,
            created_at: "2026-03-31T22:33:00Z".to_string(),
            source: "distilled-import".to_string(),
            tags: vec!["layers".to_string(), "memory".to_string()],
            archived: false,
            metadata: None,
            payload: ProjectRecordPayload::Decision(Decision {
                slug: "curated-memory-first".to_string(),
                title: "Curated memory first".to_string(),
                summary: "Curated memory should be the preferred retrieval source.".to_string(),
                rationale: "It is canonical.".to_string(),
            }),
        };
        let task = ProjectRecord {
            id: "pm_task_layers_curated".to_string(),
            entity: "task".to_string(),
            project: "layers".to_string(),
            task: Some("curated".to_string()),
            created_at: "2026-03-31T22:33:01Z".to_string(),
            source: "manual".to_string(),
            tags: vec!["layers".to_string()],
            archived: false,
            metadata: None,
            payload: ProjectRecordPayload::Task(Task {
                slug: "curated".to_string(),
                title: "Task record".to_string(),
                summary: "Curated task implementation work.".to_string(),
                status: "todo".to_string(),
                priority: None,
                acceptance: None,
            }),
        };
        append_jsonl(&curated_path, &serde_json::to_value(decision).unwrap()).unwrap();
        append_jsonl(&curated_path, &serde_json::to_value(task).unwrap()).unwrap();
        append_jsonl(
            &fallback_path,
            &serde_json::json!({
                "timestamp": "2026-03-31T22:34:00Z",
                "task": "layers memory retrospective",
                "summary": "Curated memory showed up in trace fallback too.",
                "task_type": "trace"
            }),
        )
        .unwrap();

        let (hits, issue) = search_memory("curated memory", 3).unwrap();
        assert!(issue.is_some());
        assert!(!hits.is_empty());
        assert_eq!(hits[0].source, "curated-memory");
        assert_eq!(hits[0].kind, "decision");

        if let Some(value) = original {
            unsafe {
                std::env::set_var("LAYERS_WORKSPACE_ROOT", value);
            }
        } else {
            unsafe {
                std::env::remove_var("LAYERS_WORKSPACE_ROOT");
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}
