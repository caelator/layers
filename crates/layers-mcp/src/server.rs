//! MCP server: expose Layers tools as an MCP server via stdio JSON-RPC.
//!
//! Tools are only exposed if explicitly allowlisted. Dangerous tools (those that
//! mutate state — exec, process, subagent, fs-write, edit) are blocked by
//! default unless `expose_dangerous` is set to `true` in the server config.

use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, warn};

use layers_core::{Result, ToolContext};
use layers_tools::registry::ToolRegistry;

// ---------------------------------------------------------------------------
// Dangerous-tool patterns
// ---------------------------------------------------------------------------

/// Substrings that mark a tool name as dangerous (mutating / side-effectful).
const DANGEROUS_PATTERNS: &[&str] = &[
    "exec",
    "execute",
    "run",
    "spawn",
    "process",
    "subagent",
    "write",
    "edit",
    "delete",
    "remove",
    "create",
    "mutate",
    "update",
    "kill",
    "stop",
];

/// Returns `true` if the tool name matches any dangerous pattern.
pub fn is_dangerous_tool(name: &str) -> bool {
    let lower = name.to_lowercase();
    DANGEROUS_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

// ---------------------------------------------------------------------------
// Server config
// ---------------------------------------------------------------------------

/// Configuration for the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server name announced during initialization.
    #[serde(default = "default_server_name")]
    pub server_name: String,
    /// Server version announced during initialization.
    #[serde(default = "default_server_version")]
    pub server_version: String,
    /// Explicit list of tool names to expose. Only these tools will appear in
    /// `tools/list` and be callable via `tools/call`.
    #[serde(default)]
    pub allowlisted_tools: Vec<String>,
    /// Whether to expose tools classified as dangerous. Default: `false`.
    #[serde(default)]
    pub expose_dangerous: bool,
}

fn default_server_name() -> String {
    "layers".to_string()
}

fn default_server_version() -> String {
    "0.1.0".to_string()
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            server_name: default_server_name(),
            server_version: default_server_version(),
            allowlisted_tools: Vec::new(),
            expose_dangerous: false,
        }
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC types (internal)
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
///
/// Only explicitly allowlisted tools are exposed. Dangerous tools are blocked
/// by default unless the config opts in via `expose_dangerous`.
pub struct McpServer {
    registry: Arc<ToolRegistry>,
    config: McpServerConfig,
    /// Resolved set of tool names that pass both the allowlist and the
    /// dangerous-tool policy. Built once at construction time.
    exposed: HashSet<String>,
}

impl McpServer {
    /// Create a new MCP server wrapping the given tool registry.
    pub fn new(registry: Arc<ToolRegistry>, config: McpServerConfig) -> Self {
        let exposed = Self::resolve_exposed(&registry, &config);
        Self {
            registry,
            config,
            exposed,
        }
    }

    /// Resolve the set of tool names that should be exposed.
    fn resolve_exposed(registry: &ToolRegistry, config: &McpServerConfig) -> HashSet<String> {
        let allowlist: HashSet<&str> = config
            .allowlisted_tools
            .iter()
            .map(|s| s.as_str())
            .collect();

        registry
            .names()
            .into_iter()
            .filter(|name| {
                // Must be on the explicit allowlist.
                if !allowlist.contains(name) {
                    return false;
                }
                // Block dangerous tools unless opted-in.
                if !config.expose_dangerous && is_dangerous_tool(name) {
                    warn!(
                        tool = %name,
                        "dangerous tool on allowlist but expose_dangerous=false — hidden"
                    );
                    return false;
                }
                true
            })
            .map(|s| s.to_string())
            .collect()
    }

    /// Returns the set of tool names that are actually exposed.
    pub fn exposed_tools(&self) -> &HashSet<String> {
        &self.exposed
    }

    /// Check whether a specific tool is exposed.
    pub fn is_tool_exposed(&self, name: &str) -> bool {
        self.exposed.contains(name)
    }

    /// Run the server, reading from stdin and writing to stdout.
    pub async fn run(&self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);

        debug!(
            name = %self.config.server_name,
            exposed = self.exposed.len(),
            "MCP server started"
        );

        loop {
            let mut line = String::new();
            let bytes_read = reader
                .read_line(&mut line)
                .await
                .map_err(layers_core::LayersError::Io)?;

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
                    let mut resp_line =
                        serde_json::to_string(&response).unwrap_or_default();
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

            let mut resp_line =
                serde_json::to_string(&response).unwrap_or_default();
            resp_line.push('\n');
            stdout
                .write_all(resp_line.as_bytes())
                .await
                .map_err(layers_core::LayersError::Io)?;
            stdout
                .flush()
                .await
                .map_err(layers_core::LayersError::Io)?;
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
                "name": self.config.server_name,
                "version": self.config.server_version
            }
        }))
    }

    /// Handle the `tools/list` method — only returns exposed tools.
    fn handle_tools_list(&self) -> std::result::Result<serde_json::Value, JsonRpcError> {
        let definitions = self.registry.generate_schemas();

        let tools: Vec<serde_json::Value> = definitions
            .iter()
            .filter(|def| self.exposed.contains(&def.function.name))
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

    /// Handle the `tools/call` method — only dispatches to exposed tools.
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

        // Reject tools not in the exposed set.
        if !self.exposed.contains(tool_name) {
            return Err(JsonRpcError {
                code: -32602,
                message: format!("Tool not exposed: {tool_name}"),
            });
        }

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use layers_core::{Tool, ToolContext, ToolOutput};
    use layers_tools::registry::ToolRegistry;
    use std::sync::Arc;

    // -- Dummy tools for testing -----------------------------------------------

    struct SafeTool;

    #[async_trait]
    impl Tool for SafeTool {
        fn name(&self) -> &str {
            "read_config"
        }
        fn description(&self) -> &str {
            "Read configuration (safe)"
        }
        fn schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: ToolContext,
        ) -> layers_core::Result<ToolOutput> {
            Ok(ToolOutput {
                content: "config data".to_string(),
                structured_content: None,
                attachments: vec![],
                is_error: None,
            })
        }
    }

    struct DangerousTool;

    #[async_trait]
    impl Tool for DangerousTool {
        fn name(&self) -> &str {
            "execute_command"
        }
        fn description(&self) -> &str {
            "Execute a shell command (dangerous)"
        }
        fn schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: ToolContext,
        ) -> layers_core::Result<ToolOutput> {
            Ok(ToolOutput {
                content: "command output".to_string(),
                structured_content: None,
                attachments: vec![],
                is_error: None,
            })
        }
    }

    struct AnotherSafeTool;

    #[async_trait]
    impl Tool for AnotherSafeTool {
        fn name(&self) -> &str {
            "list_models"
        }
        fn description(&self) -> &str {
            "List available models"
        }
        fn schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: ToolContext,
        ) -> layers_core::Result<ToolOutput> {
            Ok(ToolOutput {
                content: "model-a, model-b".to_string(),
                structured_content: None,
                attachments: vec![],
                is_error: None,
            })
        }
    }

    fn make_registry() -> Arc<ToolRegistry> {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(SafeTool));
        reg.register(Arc::new(DangerousTool));
        reg.register(Arc::new(AnotherSafeTool));
        Arc::new(reg)
    }

    // -- is_dangerous_tool unit tests ------------------------------------------

    #[test]
    fn dangerous_detection() {
        assert!(is_dangerous_tool("execute_command"));
        assert!(is_dangerous_tool("file_write"));
        assert!(is_dangerous_tool("spawn_process"));
        assert!(is_dangerous_tool("edit_file"));
        assert!(is_dangerous_tool("delete_record"));

        assert!(!is_dangerous_tool("read_config"));
        assert!(!is_dangerous_tool("list_models"));
        assert!(!is_dangerous_tool("query_db"));
    }

    // -- Initialize response ---------------------------------------------------

    #[test]
    fn initialize_response() {
        let registry = make_registry();
        let config = McpServerConfig {
            server_name: "test-layers".to_string(),
            server_version: "0.2.0".to_string(),
            allowlisted_tools: vec!["read_config".to_string()],
            expose_dangerous: false,
        };
        let server = McpServer::new(registry, config);

        let result = server.handle_initialize().unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "test-layers");
        assert_eq!(result["serverInfo"]["version"], "0.2.0");
        assert!(result["capabilities"]["tools"].is_object());
    }

    // -- tools/list respects allowlist -----------------------------------------

    #[test]
    fn tools_list_respects_allowlist() {
        let registry = make_registry();
        let config = McpServerConfig {
            allowlisted_tools: vec!["read_config".to_string()],
            ..Default::default()
        };
        let server = McpServer::new(registry, config);

        let result = server.handle_tools_list().unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "read_config");
    }

    #[test]
    fn tools_list_empty_allowlist_exposes_nothing() {
        let registry = make_registry();
        let config = McpServerConfig::default(); // empty allowlist
        let server = McpServer::new(registry, config);

        let result = server.handle_tools_list().unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(tools.is_empty());
    }

    // -- Dangerous tools hidden by default -------------------------------------

    #[test]
    fn dangerous_tools_hidden_by_default() {
        let registry = make_registry();
        let config = McpServerConfig {
            allowlisted_tools: vec![
                "read_config".to_string(),
                "execute_command".to_string(),
                "list_models".to_string(),
            ],
            expose_dangerous: false,
            ..Default::default()
        };
        let server = McpServer::new(registry, config);

        // execute_command is on the allowlist but dangerous — should be hidden.
        assert!(!server.is_tool_exposed("execute_command"));
        assert!(server.is_tool_exposed("read_config"));
        assert!(server.is_tool_exposed("list_models"));

        let result = server.handle_tools_list().unwrap();
        let names: Vec<&str> = result["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(!names.contains(&"execute_command"));
        assert!(names.contains(&"read_config"));
        assert!(names.contains(&"list_models"));
    }

    #[test]
    fn dangerous_tools_exposed_when_opted_in() {
        let registry = make_registry();
        let config = McpServerConfig {
            allowlisted_tools: vec![
                "read_config".to_string(),
                "execute_command".to_string(),
            ],
            expose_dangerous: true,
            ..Default::default()
        };
        let server = McpServer::new(registry, config);

        assert!(server.is_tool_exposed("execute_command"));
        assert!(server.is_tool_exposed("read_config"));
    }

    // -- tools/call dispatches allowed tools -----------------------------------

    #[tokio::test]
    async fn tools_call_dispatches_allowed() {
        let registry = make_registry();
        let config = McpServerConfig {
            allowlisted_tools: vec!["read_config".to_string()],
            ..Default::default()
        };
        let server = McpServer::new(registry, config);

        let params = serde_json::json!({
            "name": "read_config",
            "arguments": {}
        });
        let result = server.handle_tools_call(Some(&params)).await.unwrap();
        assert_eq!(result["isError"], false);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert_eq!(text, "config data");
    }

    // -- tools/call rejects blocked tools --------------------------------------

    #[tokio::test]
    async fn tools_call_rejects_non_allowlisted() {
        let registry = make_registry();
        let config = McpServerConfig {
            allowlisted_tools: vec!["read_config".to_string()],
            ..Default::default()
        };
        let server = McpServer::new(registry, config);

        let params = serde_json::json!({
            "name": "list_models",
            "arguments": {}
        });
        let err = server.handle_tools_call(Some(&params)).await.unwrap_err();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("not exposed"));
    }

    #[tokio::test]
    async fn tools_call_rejects_dangerous_when_not_opted_in() {
        let registry = make_registry();
        let config = McpServerConfig {
            allowlisted_tools: vec![
                "read_config".to_string(),
                "execute_command".to_string(),
            ],
            expose_dangerous: false,
            ..Default::default()
        };
        let server = McpServer::new(registry, config);

        let params = serde_json::json!({
            "name": "execute_command",
            "arguments": {}
        });
        let err = server.handle_tools_call(Some(&params)).await.unwrap_err();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("not exposed"));
    }

    // -- Config serde ----------------------------------------------------------

    #[test]
    fn config_default_serde() {
        let json = "{}";
        let config: McpServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.server_name, "layers");
        assert_eq!(config.server_version, "0.1.0");
        assert!(config.allowlisted_tools.is_empty());
        assert!(!config.expose_dangerous);
    }

    #[test]
    fn config_roundtrip() {
        let config = McpServerConfig {
            server_name: "custom".to_string(),
            server_version: "2.0".to_string(),
            allowlisted_tools: vec!["tool_a".to_string(), "tool_b".to_string()],
            expose_dangerous: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: McpServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.server_name, "custom");
        assert_eq!(decoded.allowlisted_tools.len(), 2);
        assert!(decoded.expose_dangerous);
    }
}
