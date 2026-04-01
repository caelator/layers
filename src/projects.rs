use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::path::Path;

use crate::config::{
    canonical_curated_memory_path, legacy_project_records_path, project_records_path,
};
use crate::types::{
    Constraint, CuratedImportRecord, Decision, MemoryHit, NextStep, Postmortem, Project,
    ProjectRecord, ProjectRecordPayload, StatusRecord, Task,
};
use crate::util::{append_jsonl, compact, iso_now, load_jsonl, tokenize};

fn normalize_slug(input: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in input.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn ensure_slug(input: &str, label: &str) -> Result<String> {
    let slug = normalize_slug(input);
    if slug.is_empty() {
        bail!("{} produced an empty slug", label);
    }
    Ok(slug)
}

fn generate_record_id(entity: &str, slug: &str) -> String {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    format!("pm_{}_{}_{}", stamp, entity, slug)
}

fn import_record_id(entity: &str, project: &str, slug: &str) -> String {
    format!("cm_{}_{}_{}", entity, project, slug)
}

fn title_from_summary(summary: &str) -> String {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return "Untitled".to_string();
    }
    let sentence = trimmed
        .split(['.', ';', '\n'])
        .next()
        .unwrap_or(trimmed)
        .trim();
    compact(sentence, 96)
}

fn record_identity(record: &ProjectRecord) -> String {
    format!(
        "{}::{}::{}",
        record.project,
        record.entity,
        record.payload.slug()
    )
}

fn is_curated_entity(entity: &str) -> bool {
    matches!(
        entity,
        "decision" | "constraint" | "status" | "next_step" | "postmortem"
    )
}

pub fn load_curated_memory() -> Result<Vec<ProjectRecord>> {
    let mut out = Vec::new();
    for item in load_jsonl(&canonical_curated_memory_path())? {
        if let Ok(record) = serde_json::from_value::<ProjectRecord>(item)
            && is_curated_entity(&record.entity)
        {
            out.push(record);
        }
    }
    Ok(out)
}

pub fn load_project_records() -> Result<Vec<ProjectRecord>> {
    let mut out = Vec::new();
    for path in [project_records_path(), legacy_project_records_path()] {
        for item in load_jsonl(&path)? {
            if let Ok(record) = serde_json::from_value::<ProjectRecord>(item) {
                out.push(record);
            }
        }
    }
    Ok(out)
}

pub fn append_curated_record(record: &ProjectRecord) -> Result<()> {
    let value = serde_json::to_value(record)?;
    append_jsonl(&canonical_curated_memory_path(), &value)
}

pub fn append_project_record(record: &ProjectRecord) -> Result<()> {
    append_curated_record(record)
}

fn curated_import_to_record(import: CuratedImportRecord) -> Result<ProjectRecord> {
    let entity = import.kind.trim().to_lowercase();
    let project = ensure_slug(&import.project, "project")?;
    let summary = import.summary.trim().to_string();
    if summary.is_empty() {
        bail!(
            "{} record for project '{}' is missing summary",
            entity,
            project
        );
    }
    let slug = ensure_slug(&summary, &format!("{} summary", entity))?;
    let title = title_from_summary(&summary);
    let payload = match entity.as_str() {
        "decision" => ProjectRecordPayload::Decision(Decision {
            slug: slug.clone(),
            title,
            summary,
            rationale: import.rationale.trim().to_string(),
        }),
        "constraint" => ProjectRecordPayload::Constraint(Constraint {
            slug: slug.clone(),
            title,
            summary,
            impact: import.rationale.trim().to_string(),
        }),
        "status" => ProjectRecordPayload::Status(StatusRecord {
            slug: slug.clone(),
            title,
            summary,
            state: if import.status.trim().is_empty() {
                "active".to_string()
            } else {
                import.status.trim().to_string()
            },
        }),
        "next_step" => ProjectRecordPayload::NextStep(NextStep {
            slug: slug.clone(),
            title,
            summary,
            owner: String::new(),
        }),
        "postmortem" => ProjectRecordPayload::Postmortem(Postmortem {
            slug: slug.clone(),
            title,
            summary,
            root_cause: import.rationale.trim().to_string(),
        }),
        _ => bail!("unsupported curated import kind: {}", entity),
    };
    Ok(ProjectRecord {
        id: import_record_id(&entity, &project, &slug),
        entity,
        project,
        task: None,
        created_at: if import.timestamp.trim().is_empty() {
            iso_now()
        } else {
            import.timestamp.trim().to_string()
        },
        source: "distilled-import".to_string(),
        tags: import.tags,
        archived: false,
        metadata: None,
        payload,
    })
}

pub fn import_curated_memory(path: &Path) -> Result<(usize, usize)> {
    let existing = load_project_records()?;
    let mut existing_keys = existing
        .into_iter()
        .map(|record| record_identity(&record))
        .collect::<std::collections::BTreeSet<_>>();
    let mut imported = 0;
    let mut skipped = 0;
    for item in load_jsonl(path)? {
        let import = serde_json::from_value::<CuratedImportRecord>(item)?;
        let record = curated_import_to_record(import)?;
        let key = record_identity(&record);
        if !existing_keys.insert(key) {
            skipped += 1;
            continue;
        }
        append_project_record(&record)?;
        imported += 1;
    }
    Ok((imported, skipped))
}

pub fn create_project(
    slug: &str,
    title: &str,
    summary: Option<&str>,
    status: Option<&str>,
) -> Result<ProjectRecord> {
    let slug = ensure_slug(slug, "project slug")?;
    let title = title.trim();
    if title.is_empty() {
        bail!("project title must not be empty");
    }
    let existing = list_projects()?;
    if existing.iter().any(|project| project.slug == slug) {
        bail!("project '{}' already exists", slug);
    }

    let payload = ProjectRecordPayload::Project(Project {
        slug: slug.clone(),
        title: title.to_string(),
        summary: summary.unwrap_or_default().trim().to_string(),
        status: status.unwrap_or("active").trim().to_string(),
    });
    let record = ProjectRecord {
        id: generate_record_id("project", payload.slug()),
        entity: payload.entity_name().to_string(),
        project: slug,
        task: None,
        created_at: iso_now(),
        source: "manual".to_string(),
        tags: vec![],
        archived: false,
        metadata: None,
        payload,
    };
    append_project_record(&record)?;
    Ok(record)
}

pub fn list_projects() -> Result<Vec<Project>> {
    let mut projects = BTreeMap::new();
    for record in load_project_records()? {
        if record.archived {
            continue;
        }
        if let ProjectRecordPayload::Project(project) = record.payload {
            projects.insert(project.slug.clone(), project);
        }
    }
    Ok(projects.into_values().collect())
}

pub fn create_task(
    project: &str,
    slug: &str,
    title: &str,
    summary: Option<&str>,
    status: Option<&str>,
    priority: Option<&str>,
    acceptance: Option<&str>,
) -> Result<ProjectRecord> {
    let project = ensure_slug(project, "project")?;
    let slug = ensure_slug(slug, "task slug")?;
    let title = title.trim();
    if title.is_empty() {
        bail!("task title must not be empty");
    }
    if !list_projects()?.iter().any(|item| item.slug == project) {
        bail!("project '{}' does not exist", project);
    }
    let existing = list_tasks(Some(&project), None)?;
    if existing.iter().any(|task| task.slug == slug) {
        bail!("task '{}' already exists in project '{}'", slug, project);
    }

    let payload = ProjectRecordPayload::Task(Task {
        slug: slug.clone(),
        title: title.to_string(),
        summary: summary.unwrap_or_default().trim().to_string(),
        status: status.unwrap_or("todo").trim().to_string(),
        priority: priority.map(|value| value.trim().to_string()),
        acceptance: acceptance.map(|value| value.trim().to_string()),
    });
    let record = ProjectRecord {
        id: generate_record_id("task", payload.slug()),
        entity: payload.entity_name().to_string(),
        project,
        task: Some(slug),
        created_at: iso_now(),
        source: "manual".to_string(),
        tags: vec![],
        archived: false,
        metadata: None,
        payload,
    };
    append_project_record(&record)?;
    Ok(record)
}

pub fn list_tasks(project: Option<&str>, status: Option<&str>) -> Result<Vec<Task>> {
    let project = project.map(|value| normalize_slug(value));
    let status = status.map(|value| value.trim().to_lowercase());
    let mut tasks = BTreeMap::new();
    for record in load_project_records()? {
        if record.archived {
            continue;
        }
        if let Some(ref wanted_project) = project
            && record.project != *wanted_project
        {
            continue;
        }
        if let ProjectRecordPayload::Task(task) = record.payload {
            if let Some(ref wanted_status) = status
                && task.status.to_lowercase() != *wanted_status
            {
                continue;
            }
            tasks.insert(format!("{}::{}", record.project, task.slug), task);
        }
    }
    Ok(tasks.into_values().collect())
}

fn record_search_text(record: &ProjectRecord) -> String {
    let mut parts = vec![
        record.entity.clone(),
        record.project.clone(),
        record.task.clone().unwrap_or_default(),
        record.payload.title().to_string(),
        record.payload.summary().to_string(),
        record.tags.join(" "),
    ];
    match &record.payload {
        ProjectRecordPayload::Project(item) => parts.push(item.status.clone()),
        ProjectRecordPayload::Task(item) => {
            parts.push(item.status.clone());
            if let Some(priority) = &item.priority {
                parts.push(priority.clone());
            }
            if let Some(acceptance) = &item.acceptance {
                parts.push(acceptance.clone());
            }
        }
        _ => {}
    }
    parts.join(" ")
}

fn structured_weight(entity: &str) -> i32 {
    match entity {
        "decision" => 8,
        "constraint" => 7,
        "next_step" => 6,
        "status" => 5,
        "task" => 4,
        "project" => 3,
        "postmortem" => 2,
        _ => 1,
    }
}

fn record_to_memory_hit(record: &ProjectRecord, score: i32, source: &str) -> MemoryHit {
    let summary = match &record.payload {
        ProjectRecordPayload::Project(item) => compact(
            &format!(
                "Project {} [{}]: {} {}",
                item.title, item.status, item.summary, record.project
            ),
            260,
        ),
        ProjectRecordPayload::Task(item) => compact(
            &format!(
                "Task {} [{}] in {}: {}",
                item.title, item.status, record.project, item.summary
            ),
            260,
        ),
        _ => compact(
            &format!("{}: {}", record.payload.title(), record.payload.summary()),
            260,
        ),
    };
    MemoryHit {
        kind: record.entity.clone(),
        score: Some(score as f64),
        timestamp: Some(record.created_at.clone()),
        task: Some(match &record.task {
            Some(task) => format!("{}::{}", record.project, task),
            None => record.project.clone(),
        }),
        summary,
        artifacts_dir: None,
        source: source.to_string(),
        graph_context: None,
    }
}

fn search_records(
    records: Vec<ProjectRecord>,
    query: &str,
    limit: usize,
    source: &str,
) -> Result<Vec<MemoryHit>> {
    let tokens = tokenize(query);
    let mut ranked = Vec::new();
    for record in records {
        if record.archived {
            continue;
        }
        let haystack = record_search_text(&record);
        let hay_tokens = tokenize(&haystack);
        let overlap = tokens.intersection(&hay_tokens).count() as i32;
        if overlap <= 0 {
            continue;
        }
        let score = overlap + structured_weight(&record.entity);
        ranked.push((score, record_to_memory_hit(&record, score, source)));
    }
    ranked.sort_by_key(|(score, hit)| {
        (
            std::cmp::Reverse(*score),
            std::cmp::Reverse(hit.timestamp.clone()),
            hit.summary.len(),
        )
    });
    Ok(ranked.into_iter().take(limit).map(|(_, hit)| hit).collect())
}

pub fn search_curated_records(query: &str, limit: usize) -> Result<Vec<MemoryHit>> {
    search_records(load_curated_memory()?, query, limit, "curated-memory")
}

pub fn search_project_records(query: &str, limit: usize) -> Result<Vec<MemoryHit>> {
    let records = load_project_records()?
        .into_iter()
        .filter(|record| matches!(record.entity.as_str(), "project" | "task"))
        .collect();
    search_records(records, query, limit, "structured-records")
}

pub fn require_project_exists(project: &str) -> Result<()> {
    let slug = ensure_slug(project, "project")?;
    if list_projects()?.iter().any(|item| item.slug == slug) {
        return Ok(());
    }
    Err(anyhow::anyhow!("project '{}' does not exist", slug))
}

pub fn canonical_project_slug(project: &str) -> Result<String> {
    ensure_slug(project, "project")
}

pub fn project_summary_line(project: &Project) -> String {
    let summary = compact(&project.summary, 100);
    if summary.is_empty() {
        format!("{} | {} | {}", project.slug, project.status, project.title)
    } else {
        format!(
            "{} | {} | {} | {}",
            project.slug, project.status, project.title, summary
        )
    }
}

pub fn task_summary_line(project: &str, task: &Task) -> String {
    let mut parts = vec![
        format!("{}::{}", project, task.slug),
        task.status.clone(),
        task.title.clone(),
    ];
    if let Some(priority) = &task.priority {
        parts.push(format!("priority={}", priority));
    }
    let summary = compact(&task.summary, 100);
    if !summary.is_empty() {
        parts.push(summary);
    }
    parts.join(" | ")
}

pub fn task_project_map(
    project: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<(String, Task)>> {
    let project_filter = project.map(|value| normalize_slug(value));
    let status_filter = status.map(|value| value.trim().to_lowercase());
    let mut tasks = BTreeMap::new();
    for record in load_project_records()? {
        if record.archived {
            continue;
        }
        if let Some(ref wanted_project) = project_filter
            && record.project != *wanted_project
        {
            continue;
        }
        if let ProjectRecordPayload::Task(task) = record.payload {
            if let Some(ref wanted_status) = status_filter
                && task.status.to_lowercase() != *wanted_status
            {
                continue;
            }
            tasks.insert(
                format!("{}::{}", record.project, task.slug),
                (record.project, task),
            );
        }
    }
    Ok(tasks.into_values().collect())
}

pub fn validate_record_shapes() -> Result<()> {
    for record in load_project_records()? {
        let payload_entity = record.payload.entity_name();
        if payload_entity != record.entity {
            bail!(
                "record {} entity mismatch: envelope={} payload={}",
                record.id,
                record.entity,
                payload_entity
            );
        }
        if record.project.trim().is_empty() {
            bail!("record {} missing project slug", record.id);
        }
        if let Some(task) = &record.task
            && task.trim().is_empty()
        {
            bail!("record {} has empty task slug", record.id);
        }
        if record.payload.slug().trim().is_empty() {
            bail!("record {} has empty payload slug", record.id);
        }
        if record.payload.title().trim().is_empty() {
            bail!("record {} has empty payload title", record.id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{curated_import_to_record, normalize_slug, search_curated_records};
    use crate::test_support::workspace_lock;
    use crate::types::CuratedImportRecord;
    use crate::types::{Decision, Project, ProjectRecord, ProjectRecordPayload, Task};
    use crate::util::append_jsonl;
    use std::fs;
    use std::path::PathBuf;

    fn temp_workspace(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "layers-tests-{}-{}",
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
    fn slug_normalization_is_stable() {
        assert_eq!(normalize_slug("Layers Native PM"), "layers-native-pm");
        assert_eq!(normalize_slug("Task: Create API"), "task-create-api");
    }

    #[test]
    fn curated_import_maps_into_structured_record() {
        let record = curated_import_to_record(CuratedImportRecord {
            kind: "decision".to_string(),
            project: "layers".to_string(),
            summary: "Canonical curated memories should be structured records.".to_string(),
            rationale: "They are auditable and stable.".to_string(),
            status: "accepted".to_string(),
            timestamp: "2026-03-31T22:33:00Z".to_string(),
            sources: vec!["chat:example".to_string()],
            tags: vec!["layers".to_string()],
        })
        .unwrap();
        assert_eq!(record.entity, "decision");
        assert_eq!(record.project, "layers");
        assert!(record.id.starts_with("cm_decision_layers_"));
        assert_eq!(record.payload.entity_name(), "decision");
        assert_eq!(
            record.payload.summary(),
            "Canonical curated memories should be structured records."
        );
    }

    #[test]
    fn curated_search_returns_only_curated_entities() {
        let _guard = workspace_lock().lock().unwrap();
        let original = std::env::var_os("LAYERS_WORKSPACE_ROOT");
        let root = temp_workspace("curated-search");
        unsafe {
            std::env::set_var("LAYERS_WORKSPACE_ROOT", &root);
        }

        let curated_path = root.join("memoryport").join("curated-memory.jsonl");
        let decision = ProjectRecord {
            id: "cm_decision_layers_gitnexus-first".to_string(),
            entity: "decision".to_string(),
            project: "layers".to_string(),
            task: None,
            created_at: "2026-03-31T22:33:00Z".to_string(),
            source: "distilled-import".to_string(),
            tags: vec!["layers".to_string(), "gitnexus".to_string()],
            archived: false,
            metadata: None,
            payload: ProjectRecordPayload::Decision(Decision {
                slug: "gitnexus-first".to_string(),
                title: "GitNexus first".to_string(),
                summary: "GitNexus should stay first-class for structural understanding."
                    .to_string(),
                rationale: "It reduces codebase guessing.".to_string(),
            }),
        };
        let task = ProjectRecord {
            id: "pm_task_layers_validate".to_string(),
            entity: "task".to_string(),
            project: "layers".to_string(),
            task: Some("validate".to_string()),
            created_at: "2026-03-31T22:33:01Z".to_string(),
            source: "manual".to_string(),
            tags: vec!["layers".to_string()],
            archived: false,
            metadata: None,
            payload: ProjectRecordPayload::Task(Task {
                slug: "validate".to_string(),
                title: "Run validate".to_string(),
                summary: "Run validate after changes.".to_string(),
                status: "todo".to_string(),
                priority: None,
                acceptance: None,
            }),
        };
        let project = ProjectRecord {
            id: "pm_project_layers".to_string(),
            entity: "project".to_string(),
            project: "layers".to_string(),
            task: None,
            created_at: "2026-03-31T22:33:02Z".to_string(),
            source: "manual".to_string(),
            tags: vec!["layers".to_string()],
            archived: false,
            metadata: None,
            payload: ProjectRecordPayload::Project(Project {
                slug: "layers".to_string(),
                title: "Layers".to_string(),
                summary: "Local-first context router.".to_string(),
                status: "active".to_string(),
            }),
        };
        append_jsonl(&curated_path, &serde_json::to_value(decision).unwrap()).unwrap();
        append_jsonl(&curated_path, &serde_json::to_value(task).unwrap()).unwrap();
        append_jsonl(&curated_path, &serde_json::to_value(project).unwrap()).unwrap();

        let hits = search_curated_records("gitnexus layers", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "decision");
        assert_eq!(hits[0].source, "curated-memory");

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
