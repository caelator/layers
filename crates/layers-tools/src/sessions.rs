//! Session management tools: list, send, spawn, history.

use serde::Deserialize;
use tracing::debug;

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};

// ---------------------------------------------------------------------------
// Sessions list
// ---------------------------------------------------------------------------

/// List active sessions.
pub struct SessionsListTool;

impl SessionsListTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for SessionsListTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for SessionsListTool {
    fn name(&self) -> &str {
        "sessions_list"
    }

    fn description(&self) -> &str {
        "List active sessions with their IDs, agent bindings, and message counts."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "Filter by agent ID"
                },
                "channel": {
                    "type": "string",
                    "description": "Filter by channel"
                }
            }
        })
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        context: ToolContext,
    ) -> Result<ToolOutput> {
        debug!(session = %context.session_id, "listing sessions");

        // Stub: requires SessionStore access at runtime.
        Ok(ToolOutput {
            content: serde_json::json!({
                "sessions": [],
                "note": "session listing requires runtime SessionStore"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Sessions send
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SessionsSendParams {
    session_id: String,
    message: String,
}

/// Send a message to an existing session.
pub struct SessionsSendTool;

impl SessionsSendTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for SessionsSendTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for SessionsSendTool {
    fn name(&self) -> &str {
        "sessions_send"
    }

    fn description(&self) -> &str {
        "Send a message to an existing session."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "Target session ID"
                },
                "message": {
                    "type": "string",
                    "description": "Message text to send"
                }
            },
            "required": ["session_id", "message"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: SessionsSendParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid sessions_send params: {e}")))?;

        debug!(target_session = %params.session_id, "sending message to session");

        // Stub: requires runtime queue manager access.
        Ok(ToolOutput {
            content: serde_json::json!({
                "session_id": params.session_id,
                "sent": true,
                "note": "session send requires runtime QueueManager"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Sessions spawn
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SessionsSpawnParams {
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
}

/// Spawn a new subagent session.
pub struct SessionsSpawnTool;

impl SessionsSpawnTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for SessionsSpawnTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for SessionsSpawnTool {
    fn name(&self) -> &str {
        "sessions_spawn"
    }

    fn description(&self) -> &str {
        "Spawn a new subagent session with an initial prompt."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Initial prompt for the subagent"
                },
                "model": {
                    "type": "string",
                    "description": "Model to use (e.g. 'anthropic/claude-sonnet-4-20250514')"
                },
                "system_prompt": {
                    "type": "string",
                    "description": "Optional system prompt override"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: SessionsSpawnParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid sessions_spawn params: {e}")))?;

        debug!(prompt_len = params.prompt.len(), "spawning subagent session");

        // Stub: requires SubagentManager access at runtime.
        let session_id = uuid::Uuid::new_v4().to_string();
        Ok(ToolOutput {
            content: serde_json::json!({
                "session_id": session_id,
                "spawned": true,
                "note": "session spawn requires runtime SubagentManager"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Sessions history
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SessionsHistoryParams {
    session_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

/// Get message history for a session.
pub struct SessionsHistoryTool;

impl SessionsHistoryTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for SessionsHistoryTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for SessionsHistoryTool {
    fn name(&self) -> &str {
        "sessions_history"
    }

    fn description(&self) -> &str {
        "Get message history for a session."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "Session ID to get history for"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of messages to return"
                }
            },
            "required": ["session_id"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: SessionsHistoryParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid sessions_history params: {e}")))?;

        debug!(
            session = %params.session_id,
            limit = ?params.limit,
            "fetching session history"
        );

        // Stub: requires SessionStore access at runtime.
        Ok(ToolOutput {
            content: serde_json::json!({
                "session_id": params.session_id,
                "messages": [],
                "note": "session history requires runtime SessionStore"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}
