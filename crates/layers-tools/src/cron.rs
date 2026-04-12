//! Cron management tools: create, list, delete cron jobs.

use serde::Deserialize;
use tracing::debug;

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};

// ---------------------------------------------------------------------------
// Cron create
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CronCreateParams {
    schedule: String,
    prompt: String,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

/// Create a new cron job.
pub struct CronCreateTool;

impl CronCreateTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for CronCreateTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for CronCreateTool {
    fn name(&self) -> &str {
        "cron_create"
    }

    fn description(&self) -> &str {
        "Create a new cron job with a schedule and prompt."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "schedule": {
                    "type": "string",
                    "description": "Cron expression (e.g. '0 9 * * *' for daily at 9am)"
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt to execute on each trigger"
                },
                "timezone": {
                    "type": "string",
                    "description": "Timezone (e.g. 'America/New_York')"
                },
                "enabled": {
                    "type": "boolean",
                    "description": "Whether the job is enabled (default: true)"
                }
            },
            "required": ["schedule", "prompt"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: CronCreateParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid cron_create params: {e}")))?;

        let id = uuid::Uuid::new_v4().to_string();
        let enabled = params.enabled.unwrap_or(true);

        debug!(id = %id, schedule = %params.schedule, enabled, "creating cron job");

        // Stub: requires cron scheduler integration.
        Ok(ToolOutput {
            content: serde_json::json!({
                "id": id,
                "schedule": params.schedule,
                "timezone": params.timezone,
                "enabled": enabled,
                "created": true
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Cron list
// ---------------------------------------------------------------------------

/// List all cron jobs.
pub struct CronListTool;

impl CronListTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for CronListTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &str {
        "cron_list"
    }

    fn description(&self) -> &str {
        "List all configured cron jobs."
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
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        debug!("listing cron jobs");

        // Stub: requires cron scheduler integration.
        Ok(ToolOutput {
            content: serde_json::json!({
                "jobs": [],
                "note": "cron listing requires cron scheduler"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Cron delete
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CronDeleteParams {
    id: String,
}

/// Delete a cron job by ID.
pub struct CronDeleteTool;

impl CronDeleteTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for CronDeleteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for CronDeleteTool {
    fn name(&self) -> &str {
        "cron_delete"
    }

    fn description(&self) -> &str {
        "Delete a cron job by its ID."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "ID of the cron job to delete"
                }
            },
            "required": ["id"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: CronDeleteParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid cron_delete params: {e}")))?;

        debug!(id = %params.id, "deleting cron job");

        // Stub: requires cron scheduler integration.
        Ok(ToolOutput {
            content: serde_json::json!({
                "id": params.id,
                "deleted": true
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}
