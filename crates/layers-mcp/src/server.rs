//! MCP server: expose Layers tools as an MCP server via stdio JSON-RPC.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error};

use layers_core::{Result, ToolContext};
use layers_tools::registry::ToolRegistry;

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

// ---------------------------------------------------------------------------
// MCP server
// ---------------------------------------------------------------------------

/// MCP server that exposes registered tools via stdio JSON-RPC.
pub struct McpServer {
    registry: Arc<ToolRegistry>,
    server_name: String,
    server_version: String,
}

impl McpServer {
    /// Create a new MCP server wrapping the given tool registry.
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self {
            registry,
            server_name: "layers".to_string(),
            server_version: "0.1.0".to_string(),
        }
    }

    /// Set the server name for capability announcements.
    #[must_use]
    pub fn with_name(mut self, name: String) -> Self {
        self.server_name = name;
        self
    }

    /// Run the server, reading from stdin and writing to stdout.
    pub async fn run(&self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);

        debug!(name = %self.server_name, "MCP server started");

        loop {
            let mut line = String::new();
            let bytes_read = reader
                .read_line(&mut line)
                .await
                .map_err(|e| layers_core::LayersError::Io(e))?;

            if bytes_read == 0 {
                debug!("MCP server: stdin closed, shutting down");
                break;
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(line) {
                Ok(r) => r,
                Err(e) => {
                    error!(error = %e, "invalid JSON-RPC request");
                    let response = JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32700,
                            message: format!("Parse error: {e}"),
                        }),
                    };
                    let mut resp_line = serde_json::to_string(&response)
                        .unwrap_or_default();
                    resp_line.push('\n');
                    let _ = stdout.write_all(resp_line.as_bytes()).await;
                    let _ = stdout.flush().await;
                    continue;
                }
            };

            let response = self.handle_request(&request).await;

            // Notifications (no id) don't get a response.
            if request.id.is_none() {
                continue;
            }

            let mut resp_line = serde_json::to_string(&response)
                .unwrap_or_default();
            resp_line.push('\n');
            stdout
                .write_all(resp_line.as_bytes())
                .await
                .map_err(|e| layers_core::LayersError::Io(e))?;
            stdout
                .flush()
                .await
                .map_err(|e| layers_core::LayersError::Io(e))?;
        }

        Ok(())
    }

    /// Handle a single JSON-RPC request.
    async fn handle_request(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(request.params.as_ref()).await,
            "notifications/initialized" => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(serde_json::json!({})),
                    error: None,
                };
            }
            _ => Err(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
            }),
        };

        match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: Some(value),
                error: None,
            },
            Err(err) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: None,
                error: Some(err),
            },
        }
    }

    /// Handle the `initialize` method.
    fn handle_initialize(&self) -> std::result::Result<serde_json::Value, JsonRpcError> {
        Ok(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": self.server_name,
                "version": self.server_version
            }
        }))
    }

    /// Handle the `tools/list` method.
    fn handle_tools_list(&self) -> std::result::Result<serde_json::Value, JsonRpcError> {
        let definitions = self.registry.generate_schemas();

        let tools: Vec<serde_json::Value> = definitions
            .iter()
            .map(|def| {
                serde_json::json!({
                    "name": def.function.name,
                    "description": def.function.description,
                    "inputSchema": def.function.parameters
                })
            })
            .collect();

        Ok(serde_json::json!({ "tools": tools }))
    }

    /// Handle the `tools/call` method.
    async fn handle_tools_call(
        &self,
        params: Option<&serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, JsonRpcError> {
        let params = params.ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Missing params".to_string(),
        })?;

        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "Missing 'name' in params".to_string(),
            })?;

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        let context = ToolContext {
            session_id: String::new(),
            agent_id: String::new(),
            channel: None,
            metadata: std::collections::HashMap::new(),
        };

        match self.registry.dispatch(tool_name, arguments, context).await {
            Ok(output) => {
                let content = vec![serde_json::json!({
                    "type": "text",
                    "text": output.content
                })];
                Ok(serde_json::json!({
                    "content": content,
                    "isError": output.is_error.unwrap_or(false)
                }))
            }
            Err(e) => {
                let content = vec![serde_json::json!({
                    "type": "text",
                    "text": e.to_string()
                })];
                Ok(serde_json::json!({
                    "content": content,
                    "isError": true
                }))
            }
        }
    }
}
