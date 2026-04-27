//! Stable product-facing MCP tools for Layers' context compiler surface.
//!
//! This module keeps the public MCP surface centered on `ContextPacket` and
//! read-only context retrieval. It deliberately does not register generic
//! runtime/process/filesystem/subagent capabilities.

use layers_core::{
    ContextBudget, ContextItem, ContextPacket, ContextSection, ContextSource, LayersError, Result,
    Tool, ToolContext, ToolOutput,
};
use layers_tools::memory::{MemoryGetTool, MemorySearchTool};
use layers_tools::registry::ToolRegistry;
use serde::Deserialize;
use uuid::Uuid;

use crate::server::STABLE_CONTEXT_SURFACE_TOOLS;

/// Build the stable product-facing MCP registry.
///
/// The registry contains only context compilation, read-only memory retrieval,
/// code-impact summarization, and context packet validation tools. Server-side
/// exposure is still controlled by `McpServerConfig::stable_context_surface()`.
#[must_use]
pub fn stable_context_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(std::sync::Arc::new(ContextCompileTool));
    registry.register(std::sync::Arc::new(ImpactAnalyzeTool));
    registry.register(std::sync::Arc::new(MemoryGetTool::new()));
    registry.register(std::sync::Arc::new(MemorySearchTool::new()));
    registry.register(std::sync::Arc::new(ValidateContextTool));
    registry
}

/// Returns `true` when a tool is part of the stable context compiler surface.
#[must_use]
pub fn is_stable_context_tool(name: &str) -> bool {
    STABLE_CONTEXT_SURFACE_TOOLS.contains(&name)
}

/// Compile an agent-ready `ContextPacket` from explicit MCP inputs.
pub struct ContextCompileTool;

#[derive(Debug, Deserialize)]
struct ContextCompileParams {
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    git_ref: Option<String>,
    #[serde(default)]
    max_units: Option<usize>,
    #[serde(default)]
    evidence: Vec<ContextEvidenceParam>,
}

#[derive(Debug, Deserialize)]
struct ContextEvidenceParam {
    title: String,
    body: String,
    #[serde(default = "default_manual_source_kind")]
    source_kind: String,
    #[serde(default = "default_manual_source_uri")]
    source_uri: String,
    #[serde(default)]
    selected_reason: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

fn default_manual_source_kind() -> String {
    "manual".to_string()
}

fn default_manual_source_uri() -> String {
    "mcp://context_compile/input".to_string()
}

#[async_trait::async_trait]
impl Tool for ContextCompileTool {
    fn name(&self) -> &str {
        "context_compile"
    }

    fn description(&self) -> &str {
        "Compile explicit task context into a stable Layers ContextPacket."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": { "type": "string", "description": "Task or query to prepare context for" },
                "query": { "type": "string", "description": "Alias for task" },
                "workspace_id": { "type": "string", "description": "Workspace/project identifier" },
                "git_ref": { "type": "string", "description": "Current git commit or ref, when known" },
                "max_units": { "type": "integer", "description": "Context budget in words/tokens" },
                "evidence": {
                    "type": "array",
                    "description": "Explicit cited snippets to include in the packet",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "body": { "type": "string" },
                            "source_kind": { "type": "string" },
                            "source_uri": { "type": "string" },
                            "selected_reason": { "type": "string" },
                            "tags": { "type": "array", "items": { "type": "string" } }
                        },
                        "required": ["title", "body"]
                    }
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, _context: ToolContext) -> Result<ToolOutput> {
        let params: ContextCompileParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid context_compile params: {e}")))?;
        let task = params
            .task
            .or(params.query)
            .unwrap_or_else(|| "Prepare coding context".to_string());
        let workspace_id = params.workspace_id.unwrap_or_else(|| "unknown".to_string());
        let mut packet = ContextPacket::new(
            format!("ctx-{}", Uuid::new_v4()),
            workspace_id,
            task,
            chrono::Utc::now(),
        );
        packet.git_ref = params.git_ref;
        packet.route = "mcp_context_compile".to_string();
        packet.confidence = "explicit".to_string();
        packet.budget = ContextBudget {
            max_units: params.max_units.unwrap_or(0),
            used_units: 0,
            unit: "words".to_string(),
            truncated: false,
        };

        if !params.evidence.is_empty() {
            let items: Vec<ContextItem> = params
                .evidence
                .into_iter()
                .enumerate()
                .map(|(idx, evidence)| {
                    let selected_reason = evidence.selected_reason.unwrap_or_else(|| {
                        "provided through stable MCP context_compile input".to_string()
                    });
                    ContextItem::cited(
                        format!("mcp-evidence-{}", idx + 1),
                        evidence.title,
                        evidence.body,
                        ContextSource::new(evidence.source_kind, evidence.source_uri),
                        selected_reason,
                    )
                    .with_tags(evidence.tags)
                })
                .collect();
            let used_units = items.iter().map(|item| item.token_estimate).sum();
            packet.budget.used_units = used_units;
            packet.sections.push(ContextSection {
                id: "mcp_input".to_string(),
                title: "MCP Input Context".to_string(),
                summary: Some("Explicit context supplied by the MCP caller.".to_string()),
                items,
            });
        }

        packet.finalize_consistency();
        Ok(json_tool_output(&packet)?)
    }
}

/// Summarize code-impact targets without invoking generic runtime tools.
pub struct ImpactAnalyzeTool;

#[derive(Debug, Deserialize)]
struct ImpactAnalyzeParams {
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    git_ref: Option<String>,
}

#[async_trait::async_trait]
impl Tool for ImpactAnalyzeTool {
    fn name(&self) -> &str {
        "impact_analyze"
    }

    fn description(&self) -> &str {
        "Summarize likely code-impact targets for a task without executing commands."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": { "type": "string", "description": "Task being assessed" },
                "targets": { "type": "array", "items": { "type": "string" }, "description": "Files/modules/features expected to be impacted" },
                "git_ref": { "type": "string", "description": "Current git commit or ref, when known" }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, _context: ToolContext) -> Result<ToolOutput> {
        let params: ImpactAnalyzeParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid impact_analyze params: {e}")))?;
        let summary = if params.targets.is_empty() {
            "No explicit impact targets were supplied; run local preflight for workspace-derived impact."
        } else {
            "Impact targets supplied explicitly by the MCP caller."
        };
        json_tool_output(&serde_json::json!({
            "task": params.task,
            "git_ref": params.git_ref,
            "summary": summary,
            "targets": params.targets,
            "requires_runtime_tools": false
        }))
    }
}

/// Validate a serialized `ContextPacket` against the stable schema.
pub struct ValidateContextTool;

#[derive(Debug, Deserialize)]
struct ValidateContextParams {
    packet: serde_json::Value,
}

#[async_trait::async_trait]
impl Tool for ValidateContextTool {
    fn name(&self) -> &str {
        "validate_context"
    }

    fn description(&self) -> &str {
        "Validate a serialized Layers ContextPacket and report schema-level issues."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "packet": { "type": "object", "description": "Serialized ContextPacket JSON" }
            },
            "required": ["packet"]
        })
    }

    async fn execute(&self, args: serde_json::Value, _context: ToolContext) -> Result<ToolOutput> {
        let params: ValidateContextParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid validate_context params: {e}")))?;
        let mut issues = Vec::new();
        match serde_json::from_value::<ContextPacket>(params.packet) {
            Ok(packet) => {
                if packet.id.trim().is_empty() {
                    issues.push("packet id is empty".to_string());
                }
                if packet.workspace_id.trim().is_empty() {
                    issues.push("workspace_id is empty".to_string());
                }
                if packet.query.trim().is_empty() {
                    issues.push("query is empty".to_string());
                }
                for warning in &packet.warnings {
                    if warning.severity.trim().is_empty() || warning.code.trim().is_empty() {
                        issues.push("warning is missing severity or code".to_string());
                    }
                }
            }
            Err(error) => issues.push(format!(
                "packet does not match ContextPacket schema: {error}"
            )),
        }
        json_tool_output(&serde_json::json!({
            "valid": issues.is_empty(),
            "issues": issues
        }))
    }
}

fn json_tool_output(value: &impl serde::Serialize) -> Result<ToolOutput> {
    let content = serde_json::to_string(value)
        .map_err(|e| LayersError::Tool(format!("failed to serialize MCP tool output: {e}")))?;
    Ok(ToolOutput {
        content,
        structured_content: None,
        attachments: Vec::new(),
        is_error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_context() -> ToolContext {
        ToolContext {
            session_id: String::new(),
            agent_id: String::new(),
            channel: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn stable_registry_contains_only_stable_surface_tools() {
        let registry = stable_context_registry();
        let mut names: Vec<&str> = registry.names();
        names.sort_unstable();
        let mut expected = STABLE_CONTEXT_SURFACE_TOOLS.to_vec();
        expected.sort_unstable();
        assert_eq!(names, expected);
    }

    #[tokio::test]
    async fn context_compile_returns_valid_context_packet() {
        let tool = ContextCompileTool;
        let output = tool
            .execute(
                serde_json::json!({
                    "task": "ship stable MCP surface",
                    "workspace_id": "layers",
                    "evidence": [{
                        "title": "Direction",
                        "body": "Expose ContextPacket, not generic runtime tools.",
                        "source_kind": "memory",
                        "source_uri": "memory://product-direction"
                    }]
                }),
                tool_context(),
            )
            .await
            .unwrap();
        let packet: ContextPacket = serde_json::from_str(&output.content).unwrap();
        assert_eq!(packet.query, "ship stable MCP surface");
        assert_eq!(packet.workspace_id, "layers");
        assert_eq!(packet.sections[0].id, "mcp_input");
        assert_eq!(packet.selection_trace[0].item_id, "mcp-evidence-1");
    }

    #[tokio::test]
    async fn validate_context_reports_schema_errors() {
        let tool = ValidateContextTool;
        let output = tool
            .execute(
                serde_json::json!({ "packet": { "id": "not-enough" } }),
                tool_context(),
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(value["valid"], false);
        assert!(
            value["issues"][0]
                .as_str()
                .unwrap()
                .contains("ContextPacket schema")
        );
    }
}
