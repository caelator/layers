//! Stable product-facing MCP tools for Layers' context compiler surface.
//!
//! This module keeps the public MCP surface centered on `ContextPacket` and
//! read-only context retrieval. It deliberately does not register generic
//! runtime/process/filesystem/subagent capabilities.

use layers_compiler::{
    CompileMode, CompileRequest, ContextCompiler, cited_item, context_section, source,
};
use layers_core::{
    ContextBudget, ContextPacket, ContextSection, LayersError, Result, Tool, ToolContext,
    ToolOutput,
};
#[cfg(feature = "vector-store")]
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
    registry.register(std::sync::Arc::new(PreflightContextTool));
    registry.register(std::sync::Arc::new(ImpactAnalyzeTool));
    #[cfg(feature = "vector-store")]
    {
        registry.register(std::sync::Arc::new(MemoryGetTool::new()));
        registry.register(std::sync::Arc::new(MemorySearchTool::new()));
    }
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
struct PreflightContextParams {
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
    targets: Vec<String>,
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

fn evidence_section(
    section_id: &str,
    section_title: &str,
    section_summary: &str,
    item_prefix: &str,
    evidence: Vec<ContextEvidenceParam>,
) -> Option<ContextSection> {
    if evidence.is_empty() {
        return None;
    }

    let items = evidence
        .into_iter()
        .enumerate()
        .map(|(idx, evidence)| {
            let selected_reason = evidence
                .selected_reason
                .unwrap_or_else(|| format!("provided through stable MCP {section_id} input"));
            cited_item(
                format!("{item_prefix}-evidence-{}", idx + 1),
                evidence.title,
                evidence.body,
                source(evidence.source_kind, evidence.source_uri),
                selected_reason,
                evidence.tags,
            )
        })
        .collect();

    Some(context_section(
        section_id,
        section_title,
        section_summary,
        items,
    ))
}

fn compile_stable_packet(
    task: String,
    workspace_id: String,
    git_ref: Option<String>,
    max_units: Option<usize>,
    mode: CompileMode,
    route_label: &str,
    sections: Vec<ContextSection>,
) -> ContextPacket {
    let used_units = sections
        .iter()
        .flat_map(|section| section.items.iter())
        .map(|item| item.token_estimate)
        .sum();
    let mut packet = ContextCompiler::new().compile(
        CompileRequest::new(
            format!("ctx-{}", Uuid::new_v4()),
            workspace_id,
            task,
            chrono::Utc::now(),
            mode,
        )
        .with_route_label(route_label)
        .with_git_ref(git_ref)
        .with_sections(sections),
    );
    packet.confidence = "explicit".to_string();
    packet.budget = ContextBudget {
        max_units: max_units.unwrap_or(0),
        used_units,
        unit: "words".to_string(),
        truncated: false,
    };
    packet
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
        let sections = evidence_section(
            "mcp_input",
            "MCP Input Context",
            "Explicit context supplied by the MCP caller.",
            "mcp",
            params.evidence,
        )
        .into_iter()
        .collect();
        let packet = compile_stable_packet(
            task,
            workspace_id,
            params.git_ref,
            params.max_units,
            CompileMode::Mcp,
            "mcp_context_compile",
            sections,
        );
        Ok(json_tool_output(&packet)?)
    }
}

/// Compile a preflight-shaped `ContextPacket` from explicit MCP inputs.
pub struct PreflightContextTool;

#[async_trait::async_trait]
impl Tool for PreflightContextTool {
    fn name(&self) -> &str {
        "preflight_context"
    }

    fn description(&self) -> &str {
        "Compile explicit preflight context into a stable Layers ContextPacket."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": { "type": "string", "description": "Task to prepare preflight context for" },
                "query": { "type": "string", "description": "Alias for task" },
                "workspace_id": { "type": "string", "description": "Workspace/project identifier" },
                "git_ref": { "type": "string", "description": "Current git commit or ref, when known" },
                "max_units": { "type": "integer", "description": "Context budget in words/tokens" },
                "targets": {
                    "type": "array",
                    "description": "Explicit files/modules/features expected to be impacted",
                    "items": { "type": "string" }
                },
                "evidence": {
                    "type": "array",
                    "description": "Explicit cited snippets to include in the preflight packet",
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
        let params: PreflightContextParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid preflight_context params: {e}")))?;
        let task = params
            .task
            .or(params.query)
            .unwrap_or_else(|| "Prepare preflight context".to_string());
        let workspace_id = params.workspace_id.unwrap_or_else(|| "unknown".to_string());
        let mut sections: Vec<ContextSection> = evidence_section(
            "preflight_input",
            "Preflight Input Context",
            "Explicit context supplied by the MCP caller for preflight.",
            "preflight",
            params.evidence,
        )
        .into_iter()
        .collect();
        if !params.targets.is_empty() {
            let items = params
                .targets
                .into_iter()
                .enumerate()
                .map(|(idx, target)| {
                    cited_item(
                        format!("preflight-target-{}", idx + 1),
                        target.clone(),
                        format!("Explicit preflight target: {target}"),
                        source("target", target),
                        "provided through stable MCP preflight_context targets".to_string(),
                        vec!["target".to_string()],
                    )
                })
                .collect();
            sections.push(context_section(
                "preflight_targets",
                "Preflight Targets",
                "Explicit impact targets supplied by the MCP caller.",
                items,
            ));
        }
        let packet = compile_stable_packet(
            task,
            workspace_id,
            params.git_ref,
            params.max_units,
            CompileMode::Preflight,
            "preflight_context",
            sections,
        );
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
    async fn context_compile_returns_compiler_finalized_context_packet() {
        let tool = ContextCompileTool;
        let output = tool
            .execute(
                serde_json::json!({
                    "task": "ship stable MCP surface",
                    "workspace_id": "layers",
                    "git_ref": "abc123",
                    "max_units": 2000,
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
        assert_eq!(packet.route, "mcp_context_compile");
        assert_eq!(packet.provenance.surface, "mcp");
        assert_eq!(packet.git_ref.as_deref(), Some("abc123"));
        assert_eq!(packet.provenance.git_ref.as_deref(), Some("abc123"));
        assert_eq!(packet.budget.max_units, 2000);
        assert_eq!(packet.sections[0].id, "mcp_input");
        assert_eq!(packet.selection_trace[0].item_id, "mcp-evidence-1");
        assert_eq!(packet.provenance.source_adapters, vec!["memory"]);
        assert!(
            packet
                .evidence
                .contains("Expose ContextPacket, not generic runtime tools.")
        );
    }

    #[tokio::test]
    async fn preflight_context_returns_compiler_finalized_context_packet() {
        let registry = stable_context_registry();
        assert!(registry.get("preflight_context").is_some());

        let tool = registry.get("preflight_context").unwrap();
        let output = tool
            .execute(
                serde_json::json!({
                    "task": "refactor packet compiler",
                    "workspace_id": "layers",
                    "git_ref": "def456",
                    "max_units": 1500,
                    "targets": ["crates/layers-mcp/src/stable.rs"],
                    "evidence": [{
                        "title": "Stable MCP",
                        "body": "MCP packet generation should go through ContextCompiler.",
                        "source_kind": "repo",
                        "source_uri": "crates/layers-mcp/src/stable.rs"
                    }]
                }),
                tool_context(),
            )
            .await
            .unwrap();
        let packet: ContextPacket = serde_json::from_str(&output.content).unwrap();
        assert_eq!(packet.query, "refactor packet compiler");
        assert_eq!(packet.workspace_id, "layers");
        assert_eq!(packet.route, "preflight_context");
        assert_eq!(packet.provenance.surface, "preflight");
        assert_eq!(packet.git_ref.as_deref(), Some("def456"));
        assert_eq!(packet.budget.max_units, 1500);
        assert_eq!(packet.sections[0].id, "preflight_input");
        assert_eq!(packet.selection_trace[0].item_id, "preflight-evidence-1");
        assert_eq!(packet.provenance.source_adapters, vec!["repo", "target"]);
        assert!(packet.evidence.contains("ContextCompiler"));
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
