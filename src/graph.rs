use anyhow::Result;
use serde_json::Value;

use crate::config::workspace_root;
use crate::types::ImpactSummary;
use crate::util::{run_command, which};

// ---------------------------------------------------------------------------
// Unified graph retrieval
// ---------------------------------------------------------------------------

fn repo_name() -> String {
    workspace_root()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string()
}

/// Query gitnexus for concepts related to `task`. Always uses --repo flag.
pub fn query(task: &str, limit: usize) -> Result<Vec<String>> {
    if which("gitnexus").is_none() {
        anyhow::bail!("gitnexus not found in PATH");
    }
    let repo = repo_name();
    let limit_str = limit.to_string();
    let args = [
        "gitnexus", "query", task, "--limit", &limit_str, "--repo", &repo,
    ];
    match run_command(&args, &workspace_root()) {
        Ok((true, stdout, _)) => {
            let trimmed = stdout.trim();
            if trimmed.is_empty() {
                return Ok(vec![]);
            }
            Ok(trimmed
                .lines()
                .take(limit)
                .map(|l| format!("- {}", l.trim()))
                .collect())
        }
        Ok((false, _, stderr)) => {
            anyhow::bail!("gitnexus query failed: {}", stderr.trim());
        }
        Err(e) => Err(e),
    }
}

/// Run impact analysis for the given symbol targets.
/// Returns None if gitnexus not available or targets is empty.
pub fn impact(targets: &[String]) -> Result<Option<ImpactSummary>> {
    if targets.is_empty() || which("gitnexus").is_none() {
        return Ok(None);
    }

    let mut all_direct = 0u64;
    let mut all_indirect = 0u64;
    let mut all_transitive = 0u64;
    let mut risk_level = String::new();
    let mut affected_processes = Vec::new();

    for target in targets {
        let args = ["gitnexus", "impact", target, "--direction", "upstream"];
        if let Ok((true, stdout, _)) = run_command(&args, &workspace_root()) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&stdout) {
                if let Some(d) = parsed.get("direct").and_then(|v| v.as_u64()) {
                    all_direct += d;
                }
                if let Some(i) = parsed.get("indirect").and_then(|v| v.as_u64()) {
                    all_indirect += i;
                }
                if let Some(t) = parsed.get("transitive").and_then(|v| v.as_u64()) {
                    all_transitive += t;
                }
                if let Some(r) = parsed.get("risk_level").and_then(|v| v.as_str()) {
                    risk_level = r.to_string();
                }
                if let Some(procs) = parsed.get("affected_processes").and_then(|v| v.as_array()) {
                    for p in procs {
                        if let Some(s) = p.as_str() {
                            if !affected_processes.contains(&s.to_string()) {
                                affected_processes.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Some(ImpactSummary {
        target_symbols: targets.to_vec(),
        blast_radius: crate::types::BlastRadius {
            direct: all_direct,
            indirect: all_indirect,
            transitive: all_transitive,
        },
        risk_level,
        affected_processes,
    }))
}
