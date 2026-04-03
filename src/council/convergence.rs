use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::types::{CouncilConvergenceRecord, CouncilRunRecord};
use crate::util::compact;

pub fn build_convergence_record(
    artifacts_dir: &Path,
    final_output: &str,
    final_output_path: &str,
) -> Result<CouncilConvergenceRecord> {
    let decision = first_bullet_after_heading(final_output, "## Decision")
        .map(|item| item.to_string())
        .unwrap_or_default();
    let why = extract_bullets_after_heading(final_output, "## Why");
    let unresolved = extract_bullets_after_heading(final_output, "## Risks");
    let next_steps = extract_bullets_after_heading(final_output, "## Next Steps");
    let mut missing_sections = Vec::new();
    if decision.is_empty() {
        missing_sections.push("decision".to_string());
    }
    if next_steps.is_empty() {
        missing_sections.push("next_steps".to_string());
    }
    let has_marker = final_output
        .to_lowercase()
        .contains("convergence: converged");
    let (status, reason) = if final_output.trim().is_empty() {
        ("not_converged".to_string(), "no_final_output".to_string())
    } else if !has_marker {
        (
            "not_converged".to_string(),
            "missing_convergence_marker".to_string(),
        )
    } else if !missing_sections.is_empty() {
        (
            "not_converged".to_string(),
            "missing_required_sections".to_string(),
        )
    } else {
        ("converged".to_string(), "converged".to_string())
    };
    let summary = compact(
        if decision.is_empty() {
            first_non_empty_line(final_output)
        } else {
            &decision
        },
        220,
    );
    let record = CouncilConvergenceRecord {
        status,
        reason,
        decision,
        summary,
        why,
        unresolved,
        next_steps,
        missing_sections,
        output_path: final_output_path.to_string(),
    };
    fs::write(
        artifacts_dir.join("convergence.json"),
        serde_json::to_string_pretty(&record)?,
    )?;
    Ok(record)
}

pub fn build_failure_convergence_record(
    artifacts_dir: &Path,
    run: &CouncilRunRecord,
) -> Result<CouncilConvergenceRecord> {
    let failed_stage = run
        .stages
        .iter()
        .find(|stage| stage.status == "failed" || stage.status == "retrying")
        .or_else(|| {
            run.stages
                .iter()
                .find(|stage| stage.status == "running" || stage.status == "pending")
        });
    let summary = failed_stage
        .map(|stage| stage.summary.clone())
        .filter(|summary| !summary.trim().is_empty())
        .unwrap_or_else(|| "council terminated before Codex convergence".to_string());
    let record = CouncilConvergenceRecord {
        status: "not_converged".to_string(),
        reason: run.status_reason.clone(),
        decision: String::new(),
        summary,
        why: vec![],
        unresolved: vec![],
        next_steps: vec![],
        missing_sections: vec!["decision".to_string(), "next_steps".to_string()],
        output_path: String::new(),
    };
    fs::write(
        artifacts_dir.join("convergence.json"),
        serde_json::to_string_pretty(&record)?,
    )?;
    Ok(record)
}

pub fn first_non_empty_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("No summary available.")
}

pub fn extract_bullets_after_heading(text: &str, heading: &str) -> Vec<String> {
    let mut in_section = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == heading {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with("## ") {
            break;
        }
        if in_section && trimmed.starts_with("- ") {
            out.push(trimmed.trim_start_matches("- ").to_string());
        }
    }
    out
}

pub fn first_bullet_after_heading<'a>(text: &'a str, heading: &str) -> Option<&'a str> {
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == heading {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with("## ") {
            break;
        }
        if in_section && trimmed.starts_with("- ") {
            return Some(trimmed.trim_start_matches("- ").trim());
        }
    }
    None
}
