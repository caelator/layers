use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

pub fn head_sha(workspace_root: &Path) -> Result<String> {
    run_git(workspace_root, ["rev-parse", "HEAD"]).map(|text| text.trim().to_string())
}

pub fn changed_files_since(workspace_root: &Path, from_sha: &str) -> Result<Vec<String>> {
    let head = head_sha(workspace_root)?;
    if from_sha == head {
        return Ok(Vec::new());
    }

    let output = run_git(
        workspace_root,
        ["diff", "--name-only", &format!("{from_sha}..HEAD")],
    )?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::string::ToString::to_string)
        .collect())
}

pub fn worktree_changed_files(workspace_root: &Path) -> Result<Vec<String>> {
    let output = run_git(
        workspace_root,
        ["status", "--porcelain", "--untracked-files=all"],
    )?;

    let mut files = BTreeSet::new();
    for line in output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
    {
        if line.len() < 4 {
            continue;
        }
        let payload = line[3..].trim();
        if let Some((before, after)) = payload.split_once(" -> ") {
            files.insert(before.trim().to_string());
            files.insert(after.trim().to_string());
        } else {
            files.insert(payload.to_string());
        }
    }
    Ok(files.into_iter().collect())
}

pub fn current_changed_files(workspace_root: &Path, from_sha: &str) -> Result<Vec<String>> {
    let mut files = BTreeSet::new();
    for file in changed_files_since(workspace_root, from_sha)? {
        files.insert(file);
    }
    for file in worktree_changed_files(workspace_root)? {
        files.insert(file);
    }
    Ok(files.into_iter().collect())
}

fn run_git<const N: usize>(workspace_root: &Path, args: [&str; N]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
