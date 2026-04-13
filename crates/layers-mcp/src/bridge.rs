//! MCP tool bridge: wraps MCP tools as `layers_core::Tool` implementations.

use std::sync::Arc;

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};

use crate::client::McpClient;
use crate::types::{ContentBlock, McpToolResult};

/// Bridges a remote MCP tool into the Layers tool system.
///
/// Names are namespaced as `mcp__{server}__{tool}` to avoid collisions.
pub struct McpToolBridge {
    server_name: String,
    tool_name: String,
    namespaced_name: String,
    description: String,
    schema: serde_json::Value,
    client: Arc<McpClient>,
}

impl McpToolBridge {
    /// Create a new bridge for a remote MCP tool.
    pub fn new(
        server_name: &str,
        tool_name: &str,
        description: &str,
        schema: serde_json::Value,
        client: Arc<McpClient>,
    ) -> Self {
        let namespaced_name = format!("mcp__{server_name}__{tool_name}");
        Self {
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
            namespaced_name,
            description: description.to_string(),
            schema,
            client,
        }
    }

    /// The original (non-namespaced) tool name.
    pub fn original_name(&self) -> &str {
        &self.tool_name
    }

    /// The server this tool belongs to.
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Parse the namespace format back into (server, tool) parts.
    pub fn parse_namespaced(name: &str) -> Option<(&str, &str)> {
        let rest = name.strip_prefix("mcp__")?;
        let (server, tool) = rest.split_once("__")?;
        if server.is_empty() || tool.is_empty() {
            return None;
        }
        Some((server, tool))
    }

    /// Convert an MCP tool result into a `ToolOutput`.
    fn convert_result(result: McpToolResult) -> ToolOutput {
        let text = result
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => text.clone(),
                ContentBlock::Image { data, mime_type } => {
                    format!("[image: {mime_type}, {} bytes]", data.len())
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        ToolOutput {
            content: text,
            structured_content: None,
            attachments: Vec::new(),
            is_error: if result.is_error { Some(true) } else { None },
        }
    }
}

#[async_trait::async_trait]
impl Tool for McpToolBridge {
    fn name(&self) -> &str {
        &self.namespaced_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let response = self.client.call_tool(&self.tool_name, args).await?;

        let mcp_result: McpToolResult = serde_json::from_value(response).map_err(|e| {
            LayersError::Tool(format!(
                "failed to parse MCP tool result from '{}': {e}",
                self.tool_name
            ))
        })?;

        Ok(Self::convert_result(mcp_result))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContentBlock, McpToolResult};

    #[test]
    fn namespacing_format() {
        let bridge = McpToolBridge {
            server_name: "github".to_string(),
            tool_name: "create_issue".to_string(),
            namespaced_name: "mcp__github__create_issue".to_string(),
            description: "Create a GitHub issue".to_string(),
            schema: serde_json::json!({"type": "object"}),
            client: Arc::new(McpClient::new_test_stub("github")),
        };

        assert_eq!(bridge.name(), "mcp__github__create_issue");
        assert_eq!(bridge.original_name(), "create_issue");
        assert_eq!(bridge.server_name(), "github");
    }

    #[test]
    fn parse_namespaced_valid() {
        let (server, tool) = McpToolBridge::parse_namespaced("mcp__slack__send_message").unwrap();
        assert_eq!(server, "slack");
        assert_eq!(tool, "send_message");
    }

    #[test]
    fn parse_namespaced_invalid() {
        assert!(McpToolBridge::parse_namespaced("not_mcp_tool").is_none());
        assert!(McpToolBridge::parse_namespaced("mcp__").is_none());
        assert!(McpToolBridge::parse_namespaced("mcp____tool").is_none());
    }

    #[test]
    fn convert_text_result() {
        let result = McpToolResult {
            content: vec![ContentBlock::Text {
                text: "success".to_string(),
            }],
            is_error: false,
        };
        let output = McpToolBridge::convert_result(result);
        assert_eq!(output.content, "success");
        assert!(output.is_error.is_none());
    }

    #[test]
    fn convert_error_result() {
        let result = McpToolResult {
            content: vec![ContentBlock::Text {
                text: "something failed".to_string(),
            }],
            is_error: true,
        };
        let output = McpToolBridge::convert_result(result);
        assert_eq!(output.content, "something failed");
        assert_eq!(output.is_error, Some(true));
    }

    #[test]
    fn convert_multipart_result() {
        let result = McpToolResult {
            content: vec![
                ContentBlock::Text {
                    text: "line 1".to_string(),
                },
                ContentBlock::Image {
                    data: "aGVsbG8=".to_string(),
                    mime_type: "image/png".to_string(),
                },
                ContentBlock::Text {
                    text: "line 2".to_string(),
                },
            ],
            is_error: false,
        };
        let output = McpToolBridge::convert_result(result);
        assert!(output.content.contains("line 1"));
        assert!(output.content.contains("line 2"));
        assert!(output.content.contains("[image: image/png"));
    }

    #[test]
    fn tool_trait_accessors() {
        let bridge = McpToolBridge {
            server_name: "test".to_string(),
            tool_name: "echo".to_string(),
            namespaced_name: "mcp__test__echo".to_string(),
            description: "Echo input".to_string(),
            schema: serde_json::json!({"type": "object", "properties": {"msg": {"type": "string"}}}),
            client: Arc::new(McpClient::new_test_stub("test")),
        };

        // Test the Tool trait methods
        let tool: &dyn Tool = &bridge;
        assert_eq!(tool.name(), "mcp__test__echo");
        assert_eq!(tool.description(), "Echo input");
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
    }
}
