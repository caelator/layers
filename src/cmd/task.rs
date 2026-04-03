use anyhow::Result;
use serde_json::json;

use crate::config::{projects_path, tasks_path};
use crate::types::{ProjectMeta, TaskMeta};
use crate::util::{append_jsonl, iso_now, load_jsonl};

pub fn handle_task_create(
    project: &str,
    slug: &str,
    title: &str,
    summary: Option<String>,
    status: Option<String>,
    json_out: bool,
) -> Result<()> {
    // Validate that the project exists
    let projects: Vec<ProjectMeta> = load_jsonl(&projects_path())?
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    if !projects.iter().any(|p| p.slug == project) {
        anyhow::bail!("project '{}' not found", project);
    }

    let id = format!("task_{}_{}", project, slug);
    let record = TaskMeta {
        id: id.clone(),
        project: project.to_string(),
        slug: slug.to_string(),
        title: title.to_string(),
        summary: summary.unwrap_or_default(),
        status: status.unwrap_or_else(|| "open".to_string()),
        created_at: iso_now(),
    };
    append_jsonl(&tasks_path(), &serde_json::to_value(&record)?)?;

    if json_out {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        println!("Created task '{}' in project '{}' ({})", slug, project, id);
    }
    Ok(())
}

pub fn handle_task_list(
    project_filter: Option<&str>,
    status_filter: Option<&str>,
    json_out: bool,
) -> Result<()> {
    let records: Vec<TaskMeta> = load_jsonl(&tasks_path())?
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .filter(|t: &TaskMeta| {
            project_filter.map_or(true, |p| t.project == p)
                && status_filter.map_or(true, |s| t.status == s)
        })
        .collect();

    if json_out {
        println!("{}", serde_json::to_string_pretty(&json!(records))?);
    } else if records.is_empty() {
        println!("No tasks found.");
    } else {
        for t in &records {
            println!("[{}] {}/{} — {}", t.status, t.project, t.slug, t.title);
            if !t.summary.is_empty() {
                println!("  {}", t.summary);
            }
        }
    }
    Ok(())
}
