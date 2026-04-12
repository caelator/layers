//! Tool registry and dispatch: register, lookup, execute, and schema generation.

use std::collections::HashMap;
use std::sync::Arc;

use tracing::debug;

use layers_core::{
    LayersError, Result, Tool, ToolContext, ToolDefinition, ToolFunction, ToolOutput,
};

// ---------------------------------------------------------------------------
// Tool profiles
// ---------------------------------------------------------------------------

/// Predefined tool profile sets.
#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub enum ToolProfile {
    /// Minimal: only essential tools.
    Minimal,
    /// Coding: file read/write, search, shell.
    Coding,
    /// Messaging: send messages, react, thread management.
    Messaging,
    /// Full: all registered tools.
    Full,
    /// Custom allow-list.
    Custom(Vec<String>),
}


// ---------------------------------------------------------------------------
// Tool registry
// ---------------------------------------------------------------------------

/// Registry of available tools with allow/deny filtering.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    allow: Option<Vec<String>>,
    deny: Vec<String>,
    profile: ToolProfile,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            allow: None,
            deny: Vec::new(),
            profile: ToolProfile::Full,
        }
    }

    pub fn with_profile(mut self, profile: ToolProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_allow(mut self, allow: Vec<String>) -> Self {
        self.allow = Some(allow);
        self
    }

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

    /// Check if a tool name is permitted by allow/deny filters.
    fn is_permitted(&self, name: &str) -> bool {
        if self.deny.iter().any(|d| d == name) {
            return false;
        }
        if let Some(ref allow) = self.allow {
            return allow.iter().any(|a| a == name);
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

    /// Generate tool definitions (JSON schema) for the model.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
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
        args: serde_json::Value,
        context: ToolContext,
    ) -> Result<ToolOutput> {
        let tool = self.get(name).ok_or_else(|| {
            LayersError::Tool(format!("tool not found or not permitted: {name}"))
        })?;

        debug!(tool = %name, "dispatching tool call");
        tool.execute(args, context).await
    }

    /// Number of registered tools (including filtered-out ones).
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
