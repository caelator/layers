//! Tool registry and dispatch: register, lookup, execute, and schema generation.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use tracing::debug;

use layers_core::{
    LayersError, Result, Tool, ToolContext, ToolDefinition, ToolFunction, ToolOutput,
};

// ---------------------------------------------------------------------------
// Tool profiles
// ---------------------------------------------------------------------------

/// Predefined tool profile sets.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ToolProfile {
    /// Minimal: only essential tools.
    Minimal,
    /// Coding: file read/write, search, shell.
    Coding,
    /// Messaging: send messages, react, thread management.
    Messaging,
    /// Full: all registered tools.
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
            Self::Coding => Some(vec!["read", "write"]),
            Self::Messaging => Some(vec!["read", "write"]),
            Self::Full => None,
            Self::Custom(names) => Some(names.iter().map(String::as_str).collect()),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Minimal => "minimal",
            Self::Coding => "coding",
            Self::Messaging => "messaging",
            Self::Full => "full",
            Self::Custom(_) => "custom",
        }
    }
}

impl fmt::Display for ToolProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Custom(names) => write!(f, "custom:{}", names.join(",")),
            _ => f.write_str(self.as_str()),
        }
    }
}

impl FromStr for ToolProfile {
    type Err = LayersError;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "minimal" => Ok(Self::Minimal),
            "coding" => Ok(Self::Coding),
            "messaging" => Ok(Self::Messaging),
            "full" => Ok(Self::Full),
            other => Err(LayersError::Config(format!(
                "unknown tool profile '{other}' (expected one of: minimal, coding, messaging, full)"
            ))),
        }
    }
}

/// Per-session policy for exposing runtime-backed tools.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolCapabilityPolicy {
    /// High-level profile baseline.
    pub profile: ToolProfile,
    /// Optional explicit allow-list that narrows the profile further.
    pub allow: Option<Vec<String>>,
    /// Explicit deny-list applied last.
    pub deny: Vec<String>,
}

impl ToolCapabilityPolicy {
    #[must_use]
    pub fn new(profile: ToolProfile) -> Self {
        Self {
            profile,
            allow: None,
            deny: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_allow(mut self, allow: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.allow = Some(allow.into_iter().map(Into::into).collect());
        self
    }

    #[must_use]
    pub fn with_deny(mut self, deny: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.deny = deny.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn allows(&self, name: &str) -> bool {
        if self.deny.iter().any(|d| d == name) {
            return false;
        }
        if let Some(ref allow) = self.allow
            && !allow.iter().any(|a| a == name)
        {
            return false;
        }
        if let Some(allowed) = self.profile.allowed_names() {
            return allowed.contains(&name);
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Tool registry
// ---------------------------------------------------------------------------

/// Registry of available tools with allow/deny filtering.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    policy: ToolCapabilityPolicy,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            policy: ToolCapabilityPolicy::new(ToolProfile::Full),
        }
    }

    pub fn with_profile(mut self, profile: ToolProfile) -> Self {
        self.policy.profile = profile;
        self
    }

    pub fn with_allow(mut self, allow: Vec<String>) -> Self {
        self.policy.allow = Some(allow);
        self
    }

    pub fn with_deny(mut self, deny: Vec<String>) -> Self {
        self.policy.deny = deny;
        self
    }

    pub fn with_policy(mut self, policy: ToolCapabilityPolicy) -> Self {
        self.policy = policy;
        self
    }

    #[must_use]
    pub fn policy(&self) -> &ToolCapabilityPolicy {
        &self.policy
    }

    /// Register a tool. Replaces any existing tool with the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        debug!(tool = %name, "registered tool");
        self.tools.insert(name, tool);
    }

    /// Check if a tool name is permitted by allow/deny filters.
    fn is_permitted(&self, name: &str) -> bool {
        self.policy.allows(name)
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
        let tool = self
            .get(name)
            .ok_or_else(|| LayersError::Tool(format!("tool not found or not permitted: {name}")))?;

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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{ToolCapabilityPolicy, ToolProfile, ToolRegistry};
    use layers_core::{Result, Tool, ToolContext, ToolOutput};
    use std::sync::Arc;

    struct MockTool {
        name: String,
    }

    impl MockTool {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "mock"
        }

        fn schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        async fn execute(
            &self,
            _args: serde_json::Value,
            _context: ToolContext,
        ) -> Result<ToolOutput> {
            Ok(ToolOutput::text("ok"))
        }
    }

    #[test]
    fn coding_profile_allows_runtime_fs_tools() {
        let mut registry = ToolRegistry::new().with_profile(ToolProfile::Coding);
        registry.register(Arc::new(MockTool::new("read")));
        registry.register(Arc::new(MockTool::new("write")));
        registry.register(Arc::new(MockTool::new("exec")));

        assert!(registry.get("read").is_some());
        assert!(registry.get("write").is_some());
        assert!(registry.get("exec").is_none());
    }

    #[test]
    fn explicit_allow_narrows_profile() {
        let mut registry = ToolRegistry::new()
            .with_policy(ToolCapabilityPolicy::new(ToolProfile::Coding).with_allow(["read"]));
        registry.register(Arc::new(MockTool::new("read")));
        registry.register(Arc::new(MockTool::new("write")));

        assert!(registry.get("read").is_some());
        assert!(registry.get("write").is_none());
    }

    #[test]
    fn explicit_deny_wins_over_profile() {
        let mut registry = ToolRegistry::new()
            .with_policy(ToolCapabilityPolicy::new(ToolProfile::Coding).with_deny(["write"]));
        registry.register(Arc::new(MockTool::new("read")));
        registry.register(Arc::new(MockTool::new("write")));

        assert!(registry.get("read").is_some());
        assert!(registry.get("write").is_none());
    }

    #[test]
    fn parses_builtin_tool_profiles_from_cli_strings() {
        assert_eq!(
            ToolProfile::from_str("minimal").expect("minimal"),
            ToolProfile::Minimal
        );
        assert_eq!(
            ToolProfile::from_str("coding").expect("coding"),
            ToolProfile::Coding
        );
        assert_eq!(
            ToolProfile::from_str("messaging").expect("messaging"),
            ToolProfile::Messaging
        );
        assert_eq!(
            ToolProfile::from_str("full").expect("full"),
            ToolProfile::Full
        );
    }

    #[test]
    fn rejects_unknown_tool_profile_strings() {
        let err = ToolProfile::from_str("danger-zone").expect_err("invalid profile");
        assert!(err.to_string().contains("unknown tool profile"));
    }
}
