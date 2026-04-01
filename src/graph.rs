use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::process::Command;

use crate::config::workspace_root;
use crate::types::{BlastRadius, GitNexusIndexVersion, ImpactSummary};
use crate::util::{compact, run_command, which};

fn gitnexus_repo_name() -> Option<String> {
    let (ok, stdout, _) = run_command(&["gitnexus", "list"], &workspace_root()).ok()?;
    if !ok {
        return None;
    }
    let root_text = workspace_root().to_string_lossy().to_string();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || !line.contains(&root_text) {
            continue;
        }
        if let Some((name, _)) = line.split_once('→') {
            return Some(name.trim().to_string());
        }
        if let Some((name, _)) = line.split_once("->") {
            return Some(name.trim().to_string());
        }
    }
    workspace_root()
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
}

fn normalize_gitnexus_status(stdout: &str, stderr: &str) -> String {
    let stdout = stdout.trim();
    let stderr = stderr.trim();
    let unborn_repo_head_error = stderr.contains("ambiguous argument 'HEAD'");

    let stderr_lines: Vec<&str> = stderr
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            if unborn_repo_head_error
                && (line.contains("ambiguous argument 'HEAD'")
                    || line.contains("unknown revision or path not in the working tree")
                    || line.starts_with("Use '--' to separate paths from revisions")
                    || line.starts_with("'git <command> [<revision>...] -- [<file>...]'"))
            {
                return None;
            }
            Some(line)
        })
        .collect();

    let mut sections = Vec::new();
    if !stdout.is_empty() {
        sections.push(stdout.to_string());
    }
    if unborn_repo_head_error {
        sections.push(
            "Commit comparison unavailable until the repository has a first commit.".to_string(),
        );
    }
    if !stderr_lines.is_empty() {
        sections.push(stderr_lines.join("\n"));
    }

    compact(&sections.join("\n"), 400)
}

pub fn gitnexus_indexed() -> Result<(bool, String, Option<String>)> {
    if which("gitnexus").is_none() {
        return Ok((false, "gitnexus not installed".to_string(), None));
    }
    let (ok, stdout, stderr) = run_command(&["gitnexus", "status"], &workspace_root())?;
    let status = normalize_gitnexus_status(&stdout, &stderr);
    let status_lower = status.to_lowercase();
    if !ok {
        return Ok((false, status, None));
    }
    if status_lower.contains("not indexed") || status_lower.contains("no index") {
        return Ok((false, status, None));
    }
    if !workspace_root().join(".gitnexus").exists() {
        return Ok((false, "no .gitnexus directory in repo".to_string(), None));
    }
    Ok((true, status, gitnexus_repo_name()))
}

pub fn gitnexus_index_version() -> Result<Option<GitNexusIndexVersion>> {
    let meta_path = workspace_root().join(".gitnexus").join("meta.json");
    if !meta_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(meta_path)?;
    let parsed: Value = serde_json::from_str(&raw)?;
    Ok(Some(GitNexusIndexVersion {
        indexed_at: parsed
            .get("indexedAt")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        last_commit: parsed
            .get("lastCommit")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        stats: parsed.get("stats").cloned().unwrap_or(Value::Null),
    }))
}

pub fn impact_summary(targets: &[String]) -> Result<Option<ImpactSummary>> {
    let (indexed, _, repo) = gitnexus_indexed()?;
    if !indexed || targets.is_empty() {
        return Ok(None);
    }

    let mut blast_radius = BlastRadius::default();
    let mut overall_risk = "low".to_string();
    let mut affected_processes = Vec::new();
    let repo_name = repo.or_else(gitnexus_repo_name);

    for target in targets {
        let mut cmd = Command::new("gitnexus");
        cmd.current_dir(workspace_root())
            .arg("impact")
            .arg(target)
            .arg("--direction")
            .arg("upstream");
        if let Some(ref repo_name) = repo_name {
            cmd.arg("--repo").arg(repo_name);
        }

        let output = cmd.output()?;
        if !output.status.success() {
            continue;
        }

        let parsed: Value = serde_json::from_slice(&output.stdout)?;
        let direct = parsed
            .get("summary")
            .and_then(|v| v.get("direct"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let impacted_count = parsed
            .get("impactedCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let indirect = impacted_count.saturating_sub(direct).min(impacted_count);

        blast_radius.direct += direct;
        blast_radius.indirect += indirect;
        blast_radius.transitive += impacted_count;

        let risk = parsed
            .get("risk")
            .and_then(|v| v.as_str())
            .unwrap_or("LOW")
            .to_lowercase();
        if risk_rank(&risk) > risk_rank(&overall_risk) {
            overall_risk = risk;
        }

        if let Some(processes) = parsed.get("affected_processes").and_then(|v| v.as_array()) {
            for process in processes {
                if let Some(name) = process.get("name").and_then(|v| v.as_str())
                    && !affected_processes.iter().any(|existing| existing == name)
                {
                    affected_processes.push(name.to_string());
                }
            }
        }
    }

    if blast_radius.transitive == 0 && affected_processes.is_empty() {
        return Ok(None);
    }

    Ok(Some(ImpactSummary {
        target_symbols: targets.to_vec(),
        blast_radius,
        risk_level: overall_risk,
        affected_processes,
    }))
}

fn risk_rank(level: &str) -> u8 {
    match level.to_ascii_lowercase().as_str() {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

pub fn normalize_graph_output(raw: &str, limit: usize) -> Vec<String> {
    if raw.trim().is_empty() {
        return vec![];
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
        let mut facts = Vec::new();
        if let Some(processes) = parsed.get("processes").and_then(|v| v.as_array()) {
            for item in processes {
                let summary = item
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("id").and_then(|v| v.as_str()))
                    .unwrap_or("process");
                let process_type = item
                    .get("process_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let step_count = item.get("step_count").and_then(|v| v.as_u64());
                let mut extra = Vec::new();
                if !process_type.is_empty() {
                    extra.push(process_type.to_string());
                }
                if let Some(c) = step_count {
                    extra.push(format!("{} steps", c));
                }
                let suffix = if extra.is_empty() {
                    "".to_string()
                } else {
                    format!(" ({})", extra.join(", "))
                };
                facts.push(compact(&format!("Process: {}{}", summary, suffix), 220));
            }
        }
        if let Some(defs) = parsed.get("definitions").and_then(|v| v.as_array()) {
            for item in defs {
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("symbol");
                let kind = item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("kind").and_then(|v| v.as_str()))
                    .unwrap_or("definition");
                let path = item
                    .get("filePath")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("path").and_then(|v| v.as_str()))
                    .unwrap_or("");
                let mut detail = format!("Definition: {} [{}]", name, kind);
                if !path.is_empty() {
                    detail.push_str(&format!(" in {}", path));
                }
                facts.push(compact(&detail, 220));
            }
        }
        if let Some(symbols) = parsed.get("process_symbols").and_then(|v| v.as_array()) {
            for item in symbols {
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("symbol");
                let kind = item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("kind").and_then(|v| v.as_str()))
                    .unwrap_or("symbol");
                let path = item
                    .get("filePath")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("path").and_then(|v| v.as_str()))
                    .unwrap_or("");
                let step_index = item.get("step_index").and_then(|v| v.as_i64());
                let mut detail = format!("Process symbol: {} [{}]", name, kind);
                if !path.is_empty() {
                    detail.push_str(&format!(" in {}", path));
                }
                if let Some(step) = step_index {
                    detail.push_str(&format!(" step {}", step));
                }
                facts.push(compact(&detail, 220));
            }
        }
        if !facts.is_empty() {
            facts.truncate(limit);
            return facts;
        }
    }
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| compact(l, 220))
        .take(limit)
        .collect()
}

pub fn query_graph(query: &str, limit: usize) -> Result<(Vec<String>, Option<String>)> {
    let (indexed, status, repo) = gitnexus_indexed()?;
    if !indexed {
        return Ok((vec![], Some(status)));
    }
    let mut cmd = Command::new("gitnexus");
    cmd.current_dir(workspace_root())
        .arg("query")
        .arg(query)
        .arg("--limit")
        .arg(limit.to_string());
    if let Some(repo_name) = repo {
        cmd.arg("--repo").arg(repo_name);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        return Ok((
            vec![],
            Some(compact(&String::from_utf8_lossy(&output.stderr), 400)),
        ));
    }
    Ok((
        normalize_graph_output(&String::from_utf8_lossy(&output.stdout), limit),
        None,
    ))
}
