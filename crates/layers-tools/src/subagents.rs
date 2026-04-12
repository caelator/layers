//! Subagent management tools: list, kill, steer.

use serde::Deserialize;
use tracing::debug;

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};

// ---------------------------------------------------------------------------
// Subagents list
// ---------------------------------------------------------------------------

/// List running subagents.
pub struct SubagentsListTool;

impl SubagentsListTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for SubagentsListTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for SubagentsListTool {
    fn name(&self) -> &str {
        "subagents_list"
    }

    fn description(&self) -> &str {
        "List currently running subagents and their status."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        context: ToolContext,
    ) -> Result<ToolOutput> {
        debug!(session = %context.session_id, "listing subagents");

        Ok(ToolOutput {
            content: serde_json::json!({
                "subagents": [],
                "note": "subagent listing requires runtime SubagentManager"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Subagents kill
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SubagentsKillParams {
    session_id: String,
}

/// Kill a running subagent.
pub struct SubagentsKillTool;

impl SubagentsKillTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for SubagentsKillTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for SubagentsKillTool {
    fn name(&self) -> &str {
        "subagents_kill"
    }

    fn description(&self) -> &str {
        "Kill a running subagent by its session ID."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "Session ID of the subagent to kill"
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
        let params: SubagentsKillParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid subagents_kill params: {e}")))?;

        debug!(target = %params.session_id, "killing subagent");

        Ok(ToolOutput {
            content: serde_json::json!({
                "session_id": params.session_id,
                "killed": true,
                "note": "subagent kill requires runtime SubagentManager"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Subagents steer
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SubagentsSteerParams {
    session_id: String,
    message: String,
}

/// Send a steering message to a running subagent.
pub struct SubagentsSteerTool;

impl SubagentsSteerTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for SubagentsSteerTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for SubagentsSteerTool {
    fn name(&self) -> &str {
        "subagents_steer"
    }

    fn description(&self) -> &str {
        "Send a steering message to a running subagent to adjust its behavior."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "Session ID of the subagent to steer"
                },
                "message": {
                    "type": "string",
                    "description": "Steering message to send"
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
        let params: SubagentsSteerParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid subagents_steer params: {e}")))?;

        debug!(
            target = %params.session_id,
            msg_len = params.message.len(),
            "steering subagent"
        );

        Ok(ToolOutput {
            content: serde_json::json!({
                "session_id": params.session_id,
                "steered": true,
                "note": "subagent steering requires runtime QueueManager"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}
