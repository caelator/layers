//! Shared bootstrap helpers for runtime-backed tool registration.

use std::sync::Arc;

use layers_runtime::tool_dispatch::{ToolCapabilityPolicy, ToolRegistry};

use crate::fs::{ReadTool, WriteTool};

/// Register the baseline runtime-backed tools into an existing registry.
pub fn register_runtime_tools(registry: &mut ToolRegistry) {
    registry.register(Arc::new(ReadTool::new()));
    registry.register(Arc::new(WriteTool::new()));
}

/// Build a runtime tool registry from a capability policy.
#[must_use]
pub fn runtime_registry(policy: ToolCapabilityPolicy) -> ToolRegistry {
    let mut registry = ToolRegistry::new().with_policy(policy);
    register_runtime_tools(&mut registry);
    registry
}

#[cfg(test)]
mod tests {
    use layers_runtime::tool_dispatch::{ToolCapabilityPolicy, ToolProfile};

    use super::{register_runtime_tools, runtime_registry};

    #[test]
    fn runtime_registry_applies_coding_policy() {
        let registry = runtime_registry(ToolCapabilityPolicy::new(ToolProfile::Coding));

        assert!(registry.get("read").is_some());
        assert!(registry.get("write").is_some());
        assert_eq!(registry.permitted_count(), 2);
    }

    #[test]
    fn runtime_registry_respects_deny_overrides() {
        let registry = runtime_registry(
            ToolCapabilityPolicy::new(ToolProfile::Coding).with_deny(["write"]),
        );

        assert!(registry.get("read").is_some());
        assert!(registry.get("write").is_none());
        assert_eq!(registry.permitted_count(), 1);
    }

    #[test]
    fn register_runtime_tools_populates_existing_registry() {
        let mut registry = layers_runtime::tool_dispatch::ToolRegistry::new();
        register_runtime_tools(&mut registry);

        let names = registry.names();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"write"));
    }
}
