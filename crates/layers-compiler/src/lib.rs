//! Pure context-packet compiler primitives.
//!
//! This crate is intentionally side-effect free. CLI, MCP, and future agent
//! integrations can assemble sections from their own adapters, then call these
//! helpers to normalize packet shape, citations, and compatibility evidence.

use std::path::Path;

use layers_core::{ContextItem, ContextPacket, ContextSection, ContextSource};

/// Return a stable workspace identifier from a workspace path.
#[must_use]
pub fn workspace_id(workspace: &Path) -> String {
    workspace
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string()
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

/// Build a cited context item with standard string conversion and tags.
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
    fn workspace_id_falls_back_for_paths_without_file_name() {
        assert_eq!(workspace_id(Path::new("/")), "workspace");
    }

    #[test]
    fn repo_source_preserves_repo_path_metadata() {
        let source = repo_source("workspace", "src/lib.rs", Some("src/lib.rs".to_string()));

        assert_eq!(source.kind, "workspace");
        assert_eq!(source.uri, "src/lib.rs");
        assert_eq!(source.repo_path.as_deref(), Some("src/lib.rs"));
    }

    #[test]
    fn section_helpers_build_cited_code_context() {
        let item = cited_item(
            "code-1",
            "Compiler API",
            "ContextCompiler request API",
            source("repo", "crates/layers-compiler/src/lib.rs"),
            "defines the shared compiler API",
            vec!["compiler".to_string()],
        );
        let section = code_impact_section(vec![item]);

        assert_eq!(section.id, "code");
        assert_eq!(section.title, "Code and Impact Context");
        assert_eq!(section.items[0].tags, vec!["compiler"]);
        assert_eq!(
            section.items[0].selected_reason,
            "defines the shared compiler API"
        );
    }

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
