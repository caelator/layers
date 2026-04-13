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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ToolProfile {
    /// Read-only essentials: read, session_status, memory_get.
    Minimal,
    /// Coding tools: minimal + exec, process, write, edit.
    Coding,
    /// Messaging tools: coding + sessions_send, sessions_history, cron.
    Messaging,
    /// All registered tools.
    #[default]
    Full,
    /// Custom allow-list.
    Custom(Vec<String>),
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
                "process",
                "write",
                "edit",
            ]),
            Self::Messaging => Some(vec![
                "read",
                "session_status",
                "memory_get",
                "exec",
                "process",
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
        let tool = self
            .get(name)
            .ok_or_else(|| LayersError::Tool(format!("tool not found or not permitted: {name}")))?;
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use layers_core::{Tool, ToolContext, ToolOutput};
    use std::sync::Arc;

    /// A simple mock tool for testing.
    struct MockTool {
        name: String,
        desc: String,
        schema: serde_json::Value,
    }

    impl MockTool {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                desc: format!("Mock tool: {name}"),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string" }
                    },
                    "required": ["input"]
                }),
            }
        }
    }

    #[async_trait::async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            &self.desc
        }
        fn schema(&self) -> serde_json::Value {
            self.schema.clone()
        }
        async fn execute(&self, args: serde_json::Value, _ctx: ToolContext) -> Result<ToolOutput> {
            let input = args.get("input").and_then(|v| v.as_str()).unwrap_or("");
            Ok(ToolOutput::text(format!("echo: {input}")))
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "test-session".into(),
            agent_id: "test-agent".into(),
            channel: None,
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn register_and_lookup() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(MockTool::new("read")));
        reg.register(Arc::new(MockTool::new("write")));

        assert!(reg.get("read").is_some());
        assert!(reg.get("write").is_some());
        assert!(reg.get("nonexistent").is_none());
        assert_eq!(reg.total_count(), 2);
    }

    #[tokio::test]
    async fn dispatch_executes_tool() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(MockTool::new("echo")));

        let result = reg
            .dispatch("echo", serde_json::json!({ "input": "hello" }), test_ctx())
            .await
            .unwrap();
        assert_eq!(result.content, "echo: hello");
    }

    #[tokio::test]
    async fn dispatch_unknown_tool_errors() {
        let reg = ToolRegistry::new();
        let result = reg
            .dispatch("nonexistent", serde_json::json!({}), test_ctx())
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn deny_list_filters_tools() {
        let mut reg = ToolRegistry::new().with_deny(vec!["exec".into()]);
        reg.register(Arc::new(MockTool::new("read")));
        reg.register(Arc::new(MockTool::new("exec")));

        assert!(reg.get("read").is_some());
        assert!(reg.get("exec").is_none());
        assert_eq!(reg.permitted_count(), 1);
    }

    #[test]
    fn allow_list_filters_tools() {
        let mut reg = ToolRegistry::new().with_allow(vec!["read".into()]);
        reg.register(Arc::new(MockTool::new("read")));
        reg.register(Arc::new(MockTool::new("write")));

        assert!(reg.get("read").is_some());
        assert!(reg.get("write").is_none());
    }

    #[test]
    fn profile_minimal_filters() {
        let mut reg = ToolRegistry::new().with_profile(ToolProfile::Minimal);
        reg.register(Arc::new(MockTool::new("read")));
        reg.register(Arc::new(MockTool::new("session_status")));
        reg.register(Arc::new(MockTool::new("memory_get")));
        reg.register(Arc::new(MockTool::new("exec")));
        reg.register(Arc::new(MockTool::new("write")));

        assert!(reg.get("read").is_some());
        assert!(reg.get("session_status").is_some());
        assert!(reg.get("memory_get").is_some());
        assert!(reg.get("exec").is_none());
        assert!(reg.get("write").is_none());
        assert_eq!(reg.permitted_count(), 3);
    }

    #[test]
    fn profile_coding_includes_exec_fs() {
        let mut reg = ToolRegistry::new().with_profile(ToolProfile::Coding);
        reg.register(Arc::new(MockTool::new("read")));
        reg.register(Arc::new(MockTool::new("exec")));
        reg.register(Arc::new(MockTool::new("process")));
        reg.register(Arc::new(MockTool::new("write")));
        reg.register(Arc::new(MockTool::new("edit")));
        reg.register(Arc::new(MockTool::new("sessions_send")));

        assert!(reg.get("read").is_some());
        assert!(reg.get("exec").is_some());
        assert!(reg.get("process").is_some());
        assert!(reg.get("write").is_some());
        assert!(reg.get("edit").is_some());
        assert!(reg.get("sessions_send").is_none());
    }

    #[test]
    fn profile_full_allows_all() {
        let mut reg = ToolRegistry::new().with_profile(ToolProfile::Full);
        reg.register(Arc::new(MockTool::new("anything")));
        assert!(reg.get("anything").is_some());
    }

    #[test]
    fn generate_schemas_returns_permitted_only() {
        let mut reg = ToolRegistry::new().with_deny(vec!["secret".into()]);
        reg.register(Arc::new(MockTool::new("read")));
        reg.register(Arc::new(MockTool::new("secret")));

        let schemas = reg.generate_schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].function.name, "read");
    }

    #[test]
    fn names_returns_permitted_sorted() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(MockTool::new("write")));
        reg.register(Arc::new(MockTool::new("read")));

        let names = reg.names();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"write"));
    }

    #[test]
    fn register_replaces_existing() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(MockTool::new("tool")));
        assert_eq!(reg.total_count(), 1);

        // Re-register should replace, not duplicate.
        reg.register(Arc::new(MockTool::new("tool")));
        assert_eq!(reg.total_count(), 1);
    }
}
