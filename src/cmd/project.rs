use anyhow::Result;
use serde_json::json;

use crate::config::projects_path;
use crate::types::ProjectMeta;
use crate::util::{append_jsonl, iso_now, load_jsonl};

pub fn handle_project_create(
    slug: &str,
    title: &str,
    summary: Option<String>,
    status: Option<String>,
    json_out: bool,
) -> Result<()> {
    let id = format!("project_{}", slug);
    let record = ProjectMeta {
        id: id.clone(),
        slug: slug.to_string(),
        title: title.to_string(),
        summary: summary.unwrap_or_default(),
        status: status.unwrap_or_else(|| "active".to_string()),
        created_at: iso_now(),
    };
    append_jsonl(&projects_path(), &serde_json::to_value(&record)?)?;

    if json_out {
        println!("{}", serde_json::to_string_pretty(&record)?);
    } else {
        println!("Created project '{}' ({})", slug, id);
    }
    Ok(())
}

pub fn handle_project_list(json_out: bool) -> Result<()> {
    let records: Vec<ProjectMeta> = load_jsonl(&projects_path())?
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .filter(|p: &ProjectMeta| p.status != "archived")
        .collect();

    if json_out {
        println!("{}", serde_json::to_string_pretty(&json!(records))?);
    } else if records.is_empty() {
        println!("No projects found.");
    } else {
        for p in &records {
            println!("[{}] {} — {}", p.status, p.slug, p.title);
            if !p.summary.is_empty() {
                println!("  {}", p.summary);
            }
        }
    }
    Ok(())
}
