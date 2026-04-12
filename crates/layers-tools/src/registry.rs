//! Extended tool registry with profile-based tool sets and schema generation.

use std::collections::HashMap;
use std::sync::Arc;

use tracing::debug;

use layers_core::{
    LayersError, Result, Tool, ToolContext, ToolDefinition, ToolFunction, ToolOutput,
};

// ---------------------------------------------------------------------------
// Tool profiles
// ---------------------------------------------------------------------------

/// Predefined tool profile sets that control which tools are available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolProfile {
    /// Read-only essentials: read, session_status, memory_get.
    Minimal,
    /// Coding tools: minimal + exec, process, write, edit.
    Coding,
    /// Messaging tools: coding + sessions_send, sessions_history, cron.
    Messaging,
    /// All registered tools.
    Full,
    /// Custom allow-list.
    Custom(Vec<String>),
}

impl Default for ToolProfile {
    fn default() -> Self {
        Self::Full
    }
}

impl ToolProfile {
    /// Returns the set of tool names allowed by this profile, or `None` for Full.
    fn allowed_names(&self) -> Option<Vec<&str>> {
        match self {
            Self::Minimal => Some(vec!["read", "session_status", "memory_get"]),
            Self::Coding => Some(vec![
                "read",
                "session_status",
                "memory_get",
                "exec",
                "write",
                "edit",
            ]),
            Self::Messaging => Some(vec![
                "read",
                "session_status",
                "memory_get",
                "exec",
                "write",
                "edit",
                "sessions_send",
                "sessions_history",
                "cron_create",
                "cron_list",
                "cron_delete",
            ]),
            Self::Full => None,
            Self::Custom(names) => Some(names.iter().map(String::as_str).collect()),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool registry
// ---------------------------------------------------------------------------

/// Registry of tool implementations with allow/deny filtering and profiles.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    allow: Option<Vec<String>>,
    deny: Vec<String>,
    profile: ToolProfile,
}

impl ToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            allow: None,
            deny: Vec::new(),
            profile: ToolProfile::Full,
        }
    }

    #[must_use]
    pub fn with_profile(mut self, profile: ToolProfile) -> Self {
        self.profile = profile;
        self
    }

    #[must_use]
    pub fn with_allow(mut self, allow: Vec<String>) -> Self {
        self.allow = Some(allow);
        self
    }

    #[must_use]
    pub fn with_deny(mut self, deny: Vec<String>) -> Self {
        self.deny = deny;
        self
    }

    /// Register a tool. Replaces any existing tool with the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        debug!(tool = %name, "registered tool");
        self.tools.insert(name, tool);
    }

    /// Check if a tool name is permitted by profile, allow, and deny filters.
    fn is_permitted(&self, name: &str) -> bool {
        // Deny list takes priority.
        if self.deny.iter().any(|d| d == name) {
            return false;
        }
        // Explicit allow list.
        if let Some(ref allow) = self.allow {
            if !allow.iter().any(|a| a == name) {
                return false;
            }
        }
        // Profile filter.
        if let Some(allowed) = self.profile.allowed_names() {
            if !allowed.contains(&name) {
                return false;
            }
        }
        true
    }

    /// Get a tool by name (respecting filters).
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        if !self.is_permitted(name) {
            return None;
        }
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// List all permitted tool names.
    pub fn names(&self) -> Vec<&str> {
        self.tools
            .keys()
            .filter(|name| self.is_permitted(name))
            .map(|s| s.as_str())
            .collect()
    }

    /// Generate tool definitions (JSON schemas) for model consumption.
    pub fn generate_schemas(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter(|(name, _)| self.is_permitted(name))
            .map(|(_, tool)| ToolDefinition {
                tool_type: "function".to_string(),
                function: ToolFunction {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: tool.schema(),
                },
            })
            .collect()
    }

    /// Dispatch a tool call by name.
    pub async fn dispatch(
        &self,
        name: &str,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolOutput> {
        let tool = self.get(name).ok_or_else(|| {
            LayersError::Tool(format!("tool not found or not permitted: {name}"))
        })?;
        debug!(tool = %name, "dispatching tool call");
        tool.execute(params, ctx).await
    }

    /// Total number of registered tools (including filtered-out ones).
    pub fn total_count(&self) -> usize {
        self.tools.len()
    }

    /// Number of permitted tools.
    pub fn permitted_count(&self) -> usize {
        self.names().len()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
