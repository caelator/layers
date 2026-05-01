//! Pure helpers for assembling and finalizing Layers context packets.
//!
//! Command modules own side effects (CLI output, process calls, audit writes).
//! This module owns small, deterministic packet assembly primitives so query,
//! preflight, impact, MCP, and autoresearch can converge on one packet shape.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

pub use layers_compiler::{
    cited_item, code_impact_section, context_section, finalize_packet, gitnexus_impact_section,
    repo_source, source, workspace_id,
};
use layers_core::{ContextPacket, ContextSection, ContextWarning};

pub mod query_plan;

const MAX_CHANGED_FILES: usize = 20;

/// Snapshot of local Git/worktree state used in agent-facing context packets.
#[derive(Debug, Clone)]
pub struct WorkspaceState {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    pub changed_files: Vec<String>,
    pub untracked_files: Vec<String>,
    pub changed_total: usize,
    pub untracked_total: usize,
    pub truncated: bool,
}

impl WorkspaceState {
    #[must_use]
    pub fn warnings(&self) -> Vec<ContextWarning> {
        let mut warnings = Vec::new();
        if self.dirty {
            warnings.push(ContextWarning {
                severity: "warning".to_string(),
                code: "dirty_worktree".to_string(),
                message: format!(
                    "Workspace has {} changed and {} untracked files; inspect before editing.",
                    self.changed_total, self.untracked_total
                ),
            });
        }
        if self.truncated {
            warnings.push(ContextWarning {
                severity: "warning".to_string(),
                code: "workspace_state_truncated".to_string(),
                message: format!(
                    "Workspace state lists are truncated to {MAX_CHANGED_FILES} changed/untracked files."
                ),
            });
        }
        warnings
    }
}

/// Collect a bounded local Git/worktree snapshot for context packets.
#[must_use]
pub fn collect_workspace_state(workspace: &Path) -> WorkspaceState {
    let branch = git_output(workspace, &["branch", "--show-current"]);
    let head = git_output(workspace, &["rev-parse", "--short", "HEAD"]);
    let status = git_output(workspace, &["status", "--porcelain=v1"]).unwrap_or_default();
    let mut changed_files = Vec::new();
    let mut untracked_files = Vec::new();
    for line in status.lines() {
        if line.len() < 4 {
            continue;
        }
        let path = line[3..].to_string();
        if line.starts_with("??") {
            untracked_files.push(path);
        } else {
            changed_files.push(path);
        }
    }
    let changed_total = changed_files.len();
    let untracked_total = untracked_files.len();
    let truncated = changed_total > MAX_CHANGED_FILES || untracked_total > MAX_CHANGED_FILES;
    changed_files.truncate(MAX_CHANGED_FILES);
    untracked_files.truncate(MAX_CHANGED_FILES);
    WorkspaceState {
        branch,
        head,
        dirty: changed_total > 0 || untracked_total > 0,
        changed_total,
        untracked_total,
        changed_files,
        untracked_files,
        truncated,
    }
}

pub(crate) fn git_output(workspace: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Build the standard structured workspace section.
#[must_use]
pub fn workspace_section(state: &WorkspaceState) -> ContextSection {
    let mut body = String::new();
    let _ = writeln!(
        body,
        "Branch: {}",
        state.branch.as_deref().unwrap_or("unknown")
    );
    let _ = writeln!(body, "Head: {}", state.head.as_deref().unwrap_or("unknown"));
    let _ = writeln!(body, "Dirty: {}", state.dirty);
    if !state.changed_files.is_empty() {
        body.push_str("Changed files:\n");
        for file in &state.changed_files {
            let _ = writeln!(body, "- {file}");
        }
    }
    if !state.untracked_files.is_empty() {
        body.push_str("Untracked files:\n");
        for file in &state.untracked_files {
            let _ = writeln!(body, "- {file}");
        }
    }
    ContextSection {
        id: "workspace".to_string(),
        title: "Workspace State".to_string(),
        summary: Some("Current Git/worktree context that can affect safe edits.".to_string()),
        items: vec![cited_item(
            "workspace-state-1",
            "Git worktree state",
            body,
            repo_source("workspace", "git status --porcelain=v1", None),
            "dirty or divergent workspace state changes how agents should interpret context",
            vec!["workspace".to_string(), "git".to_string()],
        )],
    }
}

/// Add the standard structured workspace section and related warnings.
pub fn add_workspace_section(packet: &mut ContextPacket, state: &WorkspaceState) {
    packet.warnings.extend(state.warnings());
    packet.sections.push(workspace_section(state));
}
