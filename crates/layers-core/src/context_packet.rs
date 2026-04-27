//! Stable context packet schema and renderers.
//!
//! A context packet is the core Layers artifact: bounded, cited context that a
//! coding agent can consume before acting.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Current stable ContextPacket schema version.
pub const CONTEXT_PACKET_SCHEMA_VERSION: u32 = 1;

/// Agent-ready bundle of selected context for one task/query.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextPacket {
    /// Stable packet schema version.
    pub schema_version: u32,
    /// Unique packet identifier.
    pub id: String,
    /// Workspace/project identifier.
    pub workspace_id: String,
    /// Original task or query.
    pub query: String,
    /// Packet creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Current git reference/commit when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// Retrieval route selected for this packet.
    pub route: String,
    /// Confidence label for routing/retrieval.
    pub confidence: String,
    /// Token/word budget metadata.
    pub budget: ContextBudget,
    /// Ordered context sections.
    pub sections: Vec<ContextSection>,
    /// Warnings about degraded, stale, incomplete, or conflicting context.
    #[serde(default)]
    pub warnings: Vec<ContextWarning>,
    /// Why specific items were selected.
    #[serde(default)]
    pub selection_trace: Vec<SelectionTraceEntry>,
    /// Operational retrieval metadata.
    pub retrieval: RetrievalReport,
    /// Transitional compatibility field for existing query JSON consumers.
    #[serde(default)]
    pub task: String,
    /// Transitional compatibility field for existing query JSON consumers.
    #[serde(default)]
    pub low_confidence_fallback: bool,
    /// Transitional compatibility field for existing query JSON consumers.
    #[serde(default)]
    pub scores: Value,
    /// Transitional compatibility field for existing query JSON consumers.
    #[serde(default)]
    pub why_retrieved: String,
    /// Transitional compatibility field for existing query JSON consumers.
    #[serde(default)]
    pub why_not_retrieved: String,
    /// Transitional compatibility field for existing query JSON consumers.
    #[serde(default)]
    pub evidence: String,
    /// Transitional compatibility field for existing query JSON consumers.
    #[serde(default)]
    pub open_uncertainty: Vec<String>,
    /// Transitional compatibility field for existing query JSON consumers.
    #[serde(default)]
    pub retrieval_meta: RetrievalReport,
}

impl ContextPacket {
    /// Construct a packet with schema defaults.
    #[must_use]
    pub fn new(id: String, workspace_id: String, query: String, created_at: DateTime<Utc>) -> Self {
        Self {
            schema_version: CONTEXT_PACKET_SCHEMA_VERSION,
            id,
            workspace_id,
            query: query.clone(),
            created_at,
            git_ref: None,
            route: "unknown".to_string(),
            confidence: "unknown".to_string(),
            budget: ContextBudget::default(),
            sections: Vec::new(),
            warnings: Vec::new(),
            selection_trace: Vec::new(),
            retrieval: RetrievalReport::default(),
            task: query.clone(),
            low_confidence_fallback: false,
            scores: Value::Null,
            why_retrieved: String::new(),
            why_not_retrieved: String::new(),
            evidence: String::new(),
            open_uncertainty: Vec::new(),
            retrieval_meta: RetrievalReport::default(),
        }
    }

    /// Render packet as Markdown for humans.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Layers Context Packet\n\n");
        out.push_str(&format!("Task: {}\n\n", self.query));
        out.push_str(&format!("Route: {}\n", self.route));
        out.push_str(&format!("Confidence: {}\n", self.confidence));
        if let Some(git_ref) = &self.git_ref {
            out.push_str(&format!("Git ref: {git_ref}\n"));
        }
        out.push('\n');

        if !self.warnings.is_empty() {
            out.push_str("## Warnings\n\n");
            for warning in &self.warnings {
                out.push_str(&format!("- [{}] {}\n", warning.severity, warning.message));
            }
            out.push('\n');
        }

        for section in &self.sections {
            out.push_str(&format!("## {}\n\n", section.title));
            if let Some(summary) = &section.summary {
                out.push_str(summary);
                out.push_str("\n\n");
            }
            for item in &section.items {
                out.push_str(&format!("### {}\n\n", item.title));
                out.push_str(&item.body);
                out.push_str("\n\n");
                out.push_str(&format!(
                    "Source: {} ({})\n\n",
                    item.source.uri, item.source.kind
                ));
                out.push_str(&format!("Selected because: {}\n\n", item.selected_reason));
            }
        }

        if !self.selection_trace.is_empty() {
            out.push_str("## Selection Trace\n\n");
            for trace in &self.selection_trace {
                out.push_str(&format!("- {}: {}\n", trace.item_id, trace.reason));
            }
        }

        out
    }

    /// Refresh selection trace entries from all section items.
    ///
    /// This is intentionally idempotent: callers can run packet finalization
    /// after each assembly phase without duplicating trace rows.
    pub fn refresh_selection_trace_from_sections(&mut self) {
        self.selection_trace = self
            .sections
            .iter()
            .flat_map(|section| section.items.iter())
            .map(|item| SelectionTraceEntry {
                item_id: item.id.clone(),
                reason: item.selected_reason.clone(),
            })
            .collect();
    }

    /// Refresh transitional open-uncertainty strings from packet warnings.
    ///
    /// This keeps the stable `warnings` list as the source of truth while
    /// preserving the legacy `open_uncertainty` JSON field for existing
    /// consumers.
    pub fn refresh_open_uncertainty_from_warnings(&mut self) {
        self.open_uncertainty = self
            .warnings
            .iter()
            .map(|warning| warning.message.clone())
            .collect();
    }

    /// Apply pure packet consistency invariants.
    ///
    /// Finalization deliberately avoids changing evidence text, scores,
    /// retrieval metadata, or other legacy compatibility fields whose exact
    /// shape is controlled by command-specific compatibility surfaces.
    pub fn finalize_consistency(&mut self) {
        self.refresh_selection_trace_from_sections();
        self.refresh_open_uncertainty_from_warnings();
    }

    /// Render packet as an agent-facing prompt block.
    #[must_use]
    pub fn to_agent_prompt(&self) -> String {
        let mut out = String::new();
        out.push_str("<layers_context_packet>\n");
        out.push_str(&format!("<task>{}</task>\n", escape_xml_text(&self.query)));
        out.push_str(&format!(
            "<route>{}</route>\n",
            escape_xml_text(&self.route)
        ));
        out.push_str(&format!(
            "<confidence>{}</confidence>\n",
            escape_xml_text(&self.confidence)
        ));
        if !self.warnings.is_empty() {
            out.push_str("<warnings>\n");
            for warning in &self.warnings {
                out.push_str(&format!(
                    "- [{}] {}\n",
                    escape_xml_text(&warning.severity),
                    escape_xml_text(&warning.message)
                ));
            }
            out.push_str("</warnings>\n");
        }
        out.push_str("<context>\n");
        for section in &self.sections {
            out.push_str(&format!("## {}\n", escape_xml_text(&section.title)));
            for item in &section.items {
                out.push_str(&format!(
                    "- {}:\n  ```text\n{}\n  ```\n  Source: {} ({})\n  Selected because: {}\n",
                    escape_xml_text(&item.title),
                    escape_xml_text(&item.body),
                    escape_xml_text(&item.source.uri),
                    escape_xml_text(&item.source.kind),
                    escape_xml_text(&item.selected_reason)
                ));
            }
        }
        out.push_str("</context>\n");
        out.push_str("</layers_context_packet>");
        out
    }
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Packet budget metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextBudget {
    /// Maximum allowed words/tokens depending on caller.
    pub max_units: usize,
    /// Units actually selected before rendering.
    pub used_units: usize,
    /// Unit name, e.g. `words` or `tokens`.
    pub unit: String,
    /// Whether output had to be truncated.
    pub truncated: bool,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_units: 0,
            used_units: 0,
            unit: "words".to_string(),
            truncated: false,
        }
    }
}

/// A logical section of packet context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextSection {
    /// Stable section identifier.
    pub id: String,
    /// Human title.
    pub title: String,
    /// Optional section summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Section items.
    pub items: Vec<ContextItem>,
}

/// One selected context item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextItem {
    /// Stable item identifier.
    pub id: String,
    /// Human title.
    pub title: String,
    /// Selected text/snippet.
    pub body: String,
    /// Source citation.
    pub source: ContextSource,
    /// Relevance/confidence score in range 0..1 when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    /// Estimated token/word cost.
    pub token_estimate: usize,
    /// Why this item was selected.
    pub selected_reason: String,
    /// Optional tags.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl ContextItem {
    /// Construct a cited context item while preserving explicit packet fields.
    #[must_use]
    pub fn cited(
        id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        source: ContextSource,
        selected_reason: impl Into<String>,
    ) -> Self {
        let body = body.into();
        let token_estimate = body.split_whitespace().count();
        Self {
            id: id.into(),
            title: title.into(),
            body,
            source,
            score: None,
            token_estimate,
            selected_reason: selected_reason.into(),
            tags: Vec::new(),
        }
    }

    /// Set a relevance/confidence score on an item under construction.
    #[must_use]
    pub fn with_score(mut self, score: Option<f32>) -> Self {
        self.score = score;
        self
    }

    /// Set tags on an item under construction.
    #[must_use]
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Override the estimated token/word cost when callers already have one.
    #[must_use]
    pub fn with_token_estimate(mut self, token_estimate: usize) -> Self {
        self.token_estimate = token_estimate;
        self
    }
}

/// Source citation for a context item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextSource {
    /// Source kind, e.g. memory, gitnexus, git, session, file, manual.
    pub kind: String,
    /// Source URI/path/record ID.
    pub uri: String,
    /// Optional repo-relative path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
    /// Optional line range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_range: Option<String>,
    /// Optional git commit/ref.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

impl ContextSource {
    /// Construct a source citation with no optional repo metadata.
    #[must_use]
    pub fn new(kind: impl Into<String>, uri: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            uri: uri.into(),
            repo_path: None,
            line_range: None,
            commit: None,
        }
    }

    /// Attach a repository-relative path to this source citation.
    #[must_use]
    pub fn with_repo_path(mut self, repo_path: impl Into<String>) -> Self {
        self.repo_path = Some(repo_path.into());
        self
    }

    /// Attach a line range to this source citation.
    #[must_use]
    pub fn with_line_range(mut self, line_range: impl Into<String>) -> Self {
        self.line_range = Some(line_range.into());
        self
    }

    /// Attach a git commit/ref to this source citation.
    #[must_use]
    pub fn with_commit(mut self, commit: impl Into<String>) -> Self {
        self.commit = Some(commit.into());
        self
    }
}

/// Warning included in a context packet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextWarning {
    /// Warning severity: info, warning, error.
    pub severity: String,
    /// Machine/human warning code.
    pub code: String,
    /// Human message.
    pub message: String,
}

/// Selection trace entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectionTraceEntry {
    /// Item ID referenced by this trace entry.
    pub item_id: String,
    /// Why it was selected.
    pub reason: String,
}

/// Retrieval metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RetrievalReport {
    /// Memory retrieval source.
    pub memory_source: String,
    /// Memory latency in milliseconds.
    pub memory_latency_ms: u64,
    /// Graph latency in milliseconds.
    pub graph_latency_ms: u64,
    /// Optional fallback/degraded reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_packet() -> ContextPacket {
        let mut packet = ContextPacket::new(
            "ctx-test".to_string(),
            "layers".to_string(),
            "What should I know before editing query?".to_string(),
            Utc::now(),
        );
        packet.route = "memory_only".to_string();
        packet.confidence = "low".to_string();
        packet.budget = ContextBudget {
            max_units: 1200,
            used_units: 12,
            unit: "words".to_string(),
            truncated: false,
        };
        packet.sections.push(ContextSection {
            id: "memory".to_string(),
            title: "Memory".to_string(),
            summary: Some("Relevant project memory".to_string()),
            items: vec![ContextItem {
                id: "memory-1".to_string(),
                title: "Decision".to_string(),
                body: "Layers is a context compiler.".to_string(),
                source: ContextSource {
                    kind: "memory".to_string(),
                    uri: "memoryport/curated-memory.jsonl#memory-1".to_string(),
                    repo_path: Some("memoryport/curated-memory.jsonl".to_string()),
                    line_range: None,
                    commit: None,
                },
                score: Some(0.9),
                token_estimate: 6,
                selected_reason: "keyword match".to_string(),
                tags: vec!["strategy".to_string()],
            }],
        });
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "degraded_memory".to_string(),
            message: "UC timed out; used keyword fallback.".to_string(),
        });
        packet.selection_trace.push(SelectionTraceEntry {
            item_id: "memory-1".to_string(),
            reason: "matched context compiler".to_string(),
        });
        packet
    }

    #[test]
    fn context_packet_round_trips_through_json() {
        let packet = sample_packet();
        let encoded = serde_json::to_string(&packet).unwrap();
        let decoded: ContextPacket = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.schema_version, CONTEXT_PACKET_SCHEMA_VERSION);
        assert_eq!(decoded.query, packet.query);
        assert_eq!(decoded.sections[0].items[0].source.kind, "memory");
        assert_eq!(decoded.warnings[0].code, "degraded_memory");
    }

    #[test]
    fn markdown_renderer_includes_sources_and_selection_reasons() {
        let rendered = sample_packet().to_markdown();
        assert!(rendered.contains("# Layers Context Packet"));
        assert!(rendered.contains("Source: memoryport/curated-memory.jsonl#memory-1 (memory)"));
        assert!(rendered.contains("Selected because: keyword match"));
    }

    #[test]
    fn finalize_consistency_is_idempotent_and_covers_all_items() {
        let mut packet = sample_packet();
        packet.sections[0].items.push(ContextItem {
            id: "memory-2".to_string(),
            title: "Second decision".to_string(),
            body: "ContextPacket is the product artifact.".to_string(),
            source: ContextSource {
                kind: "memory".to_string(),
                uri: "memoryport/curated-memory.jsonl#memory-2".to_string(),
                repo_path: Some("memoryport/curated-memory.jsonl".to_string()),
                line_range: None,
                commit: None,
            },
            score: Some(0.8),
            token_estimate: 5,
            selected_reason: "matched packet artifact".to_string(),
            tags: vec!["product".to_string()],
        });
        packet.selection_trace.push(SelectionTraceEntry {
            item_id: "stale".to_string(),
            reason: "stale trace should be replaced".to_string(),
        });
        packet.open_uncertainty = vec!["stale uncertainty should be replaced".to_string()];

        packet.finalize_consistency();
        let once = packet.clone();
        packet.finalize_consistency();

        assert_eq!(packet, once);
        assert_eq!(packet.selection_trace.len(), 2);
        assert_eq!(packet.selection_trace[0].item_id, "memory-1");
        assert_eq!(packet.selection_trace[1].item_id, "memory-2");
        assert_eq!(
            packet.open_uncertainty,
            vec!["UC timed out; used keyword fallback."]
        );
    }

    #[test]
    fn agent_prompt_renderer_wraps_context_packet() {
        let rendered = sample_packet().to_agent_prompt();
        assert!(rendered.starts_with("<layers_context_packet>"));
        assert!(rendered.contains("<task>What should I know before editing query?</task>"));
        assert!(rendered.contains("</layers_context_packet>"));
    }

    #[test]
    fn agent_prompt_renderer_escapes_xml_like_content() {
        let mut packet = sample_packet();
        packet.query = "edit </task> & <context>".to_string();
        packet.sections[0].items[0].body = "body with </context> & <tag>".to_string();

        let rendered = packet.to_agent_prompt();

        assert!(rendered.contains("edit &lt;/task&gt; &amp; &lt;context&gt;"));
        assert!(rendered.contains("body with &lt;/context&gt; &amp; &lt;tag&gt;"));
        assert!(!rendered.contains("edit </task>"));
        assert!(!rendered.contains("body with </context>"));
    }
}
