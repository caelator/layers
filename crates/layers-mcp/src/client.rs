//! MCP client: connect to stdio JSON-RPC MCP servers, discover tools, call them.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, error, warn};

use layers_core::{
    LayersError, McpServerConfig, Result, Tool, ToolContext, ToolOutput,
};

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i64,
    message: String,
}

// ---------------------------------------------------------------------------
// MCP tool definition from server
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct McpToolDef {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "inputSchema")]
    input_schema: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// MCP client
// ---------------------------------------------------------------------------

/// Client for a single MCP server connected via stdio.
pub struct McpClient {
    name: String,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<tokio::process::ChildStdin>>,
    stdout: Mutex<Option<BufReader<tokio::process::ChildStdout>>>,
    next_id: Mutex<u64>,
    tools: Mutex<Vec<McpToolDef>>,
}

impl McpClient {
    /// Spawn an MCP server process and initialize the connection.
    pub async fn connect(name: &str, config: &McpServerConfig) -> Result<Self> {
        if config.url.is_empty() {
            return Err(LayersError::Config(format!(
                "MCP server '{name}' has no URL/command configured"
            )));
        }

        // Parse the URL as a command. If it starts with "stdio://", extract the command.
        let command_str = config
            .url
            .strip_prefix("stdio://")
            .unwrap_or(&config.url);

        let parts: Vec<&str> = command_str.split_whitespace().collect();
        if parts.is_empty() {
            return Err(LayersError::Config(format!(
                "MCP server '{name}' has empty command"
            )));
        }

        let mut cmd = Command::new(parts[0]);
        if parts.len() > 1 {
            cmd.args(&parts[1..]);
        }

        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| LayersError::Tool(format!("failed to spawn MCP server '{name}': {e}")))?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);

        debug!(server = %name, "MCP server process spawned");

        let client = Self {
            name: name.to_string(),
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(stdout),
            next_id: Mutex::new(1),
            tools: Mutex::new(Vec::new()),
        };

        // Initialize the connection.
        client.initialize().await?;

        Ok(client)
    }

    /// Send the initialize handshake.
    async fn initialize(&self) -> Result<()> {
        let _response = self
            .send_request(
                "initialize",
                Some(serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "layers",
                        "version": "0.1.0"
                    }
                })),
            )
            .await?;

        // Send initialized notification (no response expected, but we'll handle it).
        self.send_notification("notifications/initialized", None)
            .await?;

        debug!(server = %self.name, "MCP connection initialized");
        Ok(())
    }

    /// Discover tools from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpRemoteTool>> {
        let response = self
            .send_request("tools/list", None)
            .await?;

        let tools_value = response
            .and_then(|v| v.get("tools").cloned())
            .unwrap_or_else(|| serde_json::json!([]));

        let tool_defs: Vec<McpToolDef> = serde_json::from_value(tools_value)
            .map_err(|e| LayersError::Tool(format!("failed to parse MCP tools: {e}")))?;

        let remote_tools: Vec<McpRemoteTool> = tool_defs
            .iter()
            .map(|def| McpRemoteTool {
                server_name: self.name.clone(),
                tool_name: def.name.clone(),
                tool_description: def
                    .description
                    .clone()
                    .unwrap_or_default(),
                tool_schema: def
                    .input_schema
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({"type": "object"})),
            })
            .collect();

        *self.tools.lock().await = tool_defs;

        debug!(
            server = %self.name,
            count = remote_tools.len(),
            "discovered MCP tools"
        );

        Ok(remote_tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let response = self
            .send_request(
                "tools/call",
                Some(serde_json::json!({
                    "name": tool_name,
                    "arguments": args
                })),
            )
            .await?;

        Ok(response.unwrap_or(serde_json::Value::Null))
    }

    /// Send a JSON-RPC request and wait for a response.
    async fn send_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<Option<serde_json::Value>> {
        let id = {
            let mut next = self.next_id.lock().await;
            let id = *next;
            *next += 1;
            id
        };

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let mut line = serde_json::to_string(&request)
            .map_err(LayersError::Serialization)?;
        line.push('\n');

        // Write to stdin.
        {
            let mut stdin = self.stdin.lock().await;
            let stdin = stdin
                .as_mut()
                .ok_or_else(|| LayersError::Tool("MCP server stdin closed".into()))?;
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| LayersError::Tool(format!("failed to write to MCP server: {e}")))?;
            stdin
                .flush()
                .await
                .map_err(|e| LayersError::Tool(format!("failed to flush MCP server stdin: {e}")))?;
        }

        // Read response from stdout.
        let mut response_line = String::new();
        {
            let mut stdout = self.stdout.lock().await;
            let stdout = stdout
                .as_mut()
                .ok_or_else(|| LayersError::Tool("MCP server stdout closed".into()))?;
            stdout
                .read_line(&mut response_line)
                .await
                .map_err(|e| LayersError::Tool(format!("failed to read from MCP server: {e}")))?;
        }

        if response_line.is_empty() {
            return Err(LayersError::Tool("MCP server closed connection".into()));
        }

        let response: JsonRpcResponse = serde_json::from_str(&response_line)
            .map_err(|e| LayersError::Tool(format!("invalid JSON-RPC response: {e}")))?;

        if let Some(err) = response.error {
            return Err(LayersError::Tool(format!(
                "MCP server error: {}",
                err.message
            )));
        }

        Ok(response.result)
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn send_notification(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(serde_json::json!({}))
        });

        let mut line = notification.to_string();
        line.push('\n');

        let mut stdin = self.stdin.lock().await;
        if let Some(stdin) = stdin.as_mut() {
            let _ = stdin.write_all(line.as_bytes()).await;
            let _ = stdin.flush().await;
        }

        Ok(())
    }

    /// Shut down the MCP server process.
    pub async fn shutdown(&self) {
        // Drop stdin to signal EOF.
        {
            let mut stdin = self.stdin.lock().await;
            *stdin = None;
        }

        // Kill the child process.
        let mut child = self.child.lock().await;
        if let Some(ref mut child) = *child {
            let _ = child.kill().await;
            debug!(server = %self.name, "MCP server shut down");
        }
        *child = None;
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // Best-effort synchronous cleanup.
        if let Ok(mut child) = self.child.try_lock() {
            if let Some(ref mut c) = *child {
                let _ = c.start_kill();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MCP remote tool (wraps a tool discovered from an MCP server)
// ---------------------------------------------------------------------------

/// A tool discovered from a remote MCP server, callable via JSON-RPC.
pub struct McpRemoteTool {
    server_name: String,
    tool_name: String,
    tool_description: String,
    tool_schema: serde_json::Value,
}

impl McpRemoteTool {
    /// Get the server name this tool belongs to.
    #[must_use]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }
}

// Note: McpRemoteTool implements the Tool trait but needs a reference to
// the McpClient to actually call the tool. In practice, the ToolRegistry
// would hold an Arc<McpClient> and dispatch calls through it.
// For compilation, we provide a standalone implementation that returns
// a stub indicating the call needs to be routed through the client.

#[async_trait::async_trait]
impl Tool for McpRemoteTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn schema(&self) -> serde_json::Value {
        self.tool_schema.clone()
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        // In real usage, this would be dispatched through the McpClient.
        // The registry should use McpDispatchTool instead for live connections.
        Err(LayersError::Tool(format!(
            "MCP tool '{}' from server '{}' must be called via McpClient",
            self.tool_name, self.server_name
        )))
    }
}

// ---------------------------------------------------------------------------
// MCP manager: manages multiple MCP server connections
// ---------------------------------------------------------------------------

/// Manages connections to multiple MCP servers.
pub struct McpManager {
    clients: HashMap<String, Arc<McpClient>>,
}

impl McpManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    /// Connect to all configured MCP servers.
    pub async fn connect_all(
        configs: &HashMap<String, McpServerConfig>,
    ) -> Result<Self> {
        let mut clients = HashMap::new();

        for (name, config) in configs {
            match McpClient::connect(name, config).await {
                Ok(client) => {
                    clients.insert(name.clone(), Arc::new(client));
                }
                Err(e) => {
                    warn!(server = %name, error = %e, "failed to connect to MCP server");
                }
            }
        }

        Ok(Self { clients })
    }

    /// Get a client by server name.
    pub fn get(&self, name: &str) -> Option<&Arc<McpClient>> {
        self.clients.get(name)
    }

    /// Discover tools from all connected servers.
    pub async fn discover_all_tools(&self) -> Result<Vec<McpRemoteTool>> {
        let mut all_tools = Vec::new();

        for (name, client) in &self.clients {
            match client.list_tools().await {
                Ok(tools) => all_tools.extend(tools),
                Err(e) => {
                    error!(server = %name, error = %e, "failed to list MCP tools");
                }
            }
        }

        Ok(all_tools)
    }

    /// Shut down all connections.
    pub async fn shutdown_all(&self) {
        for client in self.clients.values() {
            client.shutdown().await;
        }
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}
