//! Pure helpers for assembling and finalizing Layers context packets.
//!
//! Command modules own side effects (CLI output, process calls, audit writes).
//! This module owns small, deterministic packet assembly primitives so query,
//! preflight, impact, MCP, and autoresearch can converge on one packet shape.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use layers_core::{ContextItem, ContextPacket, ContextSection, ContextSource, ContextWarning};

pub mod query_plan;

const MAX_CHANGED_FILES: usize = 20;

/// Return a stable workspace identifier from a workspace path.
#[must_use]
pub fn workspace_id(workspace: &Path) -> String {
    workspace
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string()
}

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

/// Build a context section with standard string conversion.
#[must_use]
pub fn context_section(
    id: impl Into<String>,
    title: impl Into<String>,
    summary: impl Into<String>,
    items: Vec<ContextItem>,
) -> ContextSection {
    ContextSection {
        id: id.into(),
        title: title.into(),
        summary: Some(summary.into()),
        items,
    }
}

/// Build the standard code/impact context section used by local preflight flows.
#[must_use]
pub fn code_impact_section(items: Vec<ContextItem>) -> ContextSection {
    context_section(
        "code",
        "Code and Impact Context",
        "Local code context gathered without editing files.",
        items,
    )
}

/// Build the standard `GitNexus` structural-impact section.
#[must_use]
pub fn gitnexus_impact_section(items: Vec<ContextItem>) -> ContextSection {
    context_section(
        "gitnexus",
        "GitNexus",
        "Relevant code graph and structural context.",
        items,
    )
}

#[must_use]
pub fn cited_item(
    id: impl Into<String>,
    title: impl Into<String>,
    body: impl Into<String>,
    source: ContextSource,
    selected_reason: impl Into<String>,
    tags: Vec<String>,
) -> ContextItem {
    ContextItem::cited(id, title, body, source, selected_reason).with_tags(tags)
}

/// Build a basic source citation.
#[must_use]
pub fn source(kind: impl Into<String>, uri: impl Into<String>) -> ContextSource {
    ContextSource::new(kind, uri)
}

/// Build a source citation that carries repo-relative path metadata.
#[must_use]
pub fn repo_source(
    kind: impl Into<String>,
    uri: impl Into<String>,
    repo_path: Option<String>,
) -> ContextSource {
    let mut source = ContextSource::new(kind, uri);
    source.repo_path = repo_path;
    source
}

/// Render legacy evidence text from packet sections/items.
#[must_use]
pub fn render_section_evidence(packet: &ContextPacket) -> String {
    packet
        .sections
        .iter()
        .filter(|section| !section.items.is_empty())
        .map(|section| {
            let rendered_items = section
                .items
                .iter()
                .map(|item| {
                    format!(
                        "{}: {}\n- [{}] {}\nSource: {} ({})\nSelected because: {}",
                        section.title,
                        item.title,
                        item.source.uri,
                        item.body,
                        item.source.uri,
                        item.source.kind,
                        item.selected_reason
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            format!("### {}\n{}", section.title, rendered_items)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Finalize packet consistency while optionally deriving legacy evidence from sections.
pub fn finalize_packet(packet: &mut ContextPacket, derive_evidence: bool) {
    packet.finalize_consistency();
    if derive_evidence {
        packet.evidence = render_section_evidence(packet);
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use layers_core::{ContextPacket, ContextSection};

    use super::*;

    #[test]
    fn render_section_evidence_preserves_source_and_selection_reason() {
        let mut packet = ContextPacket::new(
            "ctx-test".to_string(),
            "layers".to_string(),
            "compile context".to_string(),
            Utc::now(),
        );
        packet.sections.push(ContextSection {
            id: "memory".to_string(),
            title: "Memory".to_string(),
            summary: None,
            items: vec![cited_item(
                "memory-1",
                "North star",
                "Layers is a local-first context compiler.",
                source("memory", "memoryport/curated-memory.jsonl#1"),
                "matched context compiler",
                vec!["strategy".to_string()],
            )],
        });

        let evidence = render_section_evidence(&packet);

        assert!(evidence.contains("Memory: North star"));
        assert!(evidence.contains("Source: memoryport/curated-memory.jsonl#1 (memory)"));
        assert!(evidence.contains("Selected because: matched context compiler"));
    }

    #[test]
    fn finalize_packet_can_derive_evidence_from_sections() {
        let mut packet = ContextPacket::new(
            "ctx-test".to_string(),
            "layers".to_string(),
            "compile context".to_string(),
            Utc::now(),
        );
        packet.sections.push(ContextSection {
            id: "autoresearch".to_string(),
            title: "Persisted Autoresearch Findings".to_string(),
            summary: None,
            items: vec![cited_item(
                "autoresearch-1",
                "Finding",
                "Freshness: current\nReliability: high\nProvenance: seeded-source",
                source("autoresearch", "seeded-source"),
                "matched task",
                vec!["autoresearch".to_string(), "provenance".to_string()],
            )],
        });

        finalize_packet(&mut packet, true);

        assert_eq!(packet.selection_trace.len(), 1);
        assert!(packet.evidence.contains("Freshness: current"));
        assert!(
            packet
                .evidence
                .contains("Source: seeded-source (autoresearch)")
        );
    }
}
