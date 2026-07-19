//! Shared bootstrap helpers for runtime-backed tool registration.

use std::collections::HashMap;
use std::sync::Arc;

use layers_core::{LayersError, Result};
use layers_runtime::tool_dispatch::{ToolCapabilityPolicy, ToolProfile, ToolRegistry};

use crate::fs::{ReadTool, WriteTool};

/// Register the baseline runtime-backed tools into an existing registry.
pub fn register_runtime_tools(registry: &mut ToolRegistry) {
    registry.register(Arc::new(ReadTool::new()));
    registry.register(Arc::new(WriteTool::new()));
}

/// Resolve a runtime-backed tool policy from CLI-friendly profile selection.
pub fn runtime_policy_from_cli(
    profile: &str,
    named_profiles: &HashMap<String, Vec<String>>,
    allow: &[String],
    deny: &[String],
) -> Result<ToolCapabilityPolicy> {
    let profile_key = profile.trim();
    let base_profile = if let Ok(profile) = profile_key.parse::<ToolProfile>() {
        profile
    } else if let Some(names) = named_profiles.get(profile_key) {
        ToolProfile::Custom(names.clone())
    } else {
        return Err(LayersError::Config(format!(
            "unknown tool profile '{profile_key}' (expected builtin minimal/coding/messaging/full or a [tools.profiles] entry)"
        )));
    };

    let mut policy = ToolCapabilityPolicy::new(base_profile);
    if !allow.is_empty() {
        policy = policy.with_allow(allow.to_vec());
    }
    if !deny.is_empty() {
        policy = policy.with_deny(deny.to_vec());
    }
    Ok(policy)
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
    use std::collections::HashMap;

    use layers_runtime::tool_dispatch::{ToolCapabilityPolicy, ToolProfile};

    use super::{register_runtime_tools, runtime_policy_from_cli, runtime_registry};

    #[test]
    fn runtime_registry_applies_coding_policy() {
        let registry = runtime_registry(ToolCapabilityPolicy::new(ToolProfile::Coding));

        assert!(registry.get("read").is_some());
        assert!(registry.get("write").is_some());
        assert_eq!(registry.permitted_count(), 2);
    }

    #[test]
    fn runtime_registry_respects_deny_overrides() {
        let registry =
            runtime_registry(ToolCapabilityPolicy::new(ToolProfile::Coding).with_deny(["write"]));

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

    #[test]
    fn runtime_policy_from_cli_supports_named_profiles() {
        let named_profiles = HashMap::from([("fs-readonly".to_string(), vec!["read".to_string()])]);

        let policy = runtime_policy_from_cli("fs-readonly", &named_profiles, &[], &[])
            .expect("named profile should resolve");

        assert_eq!(
            policy.profile,
            ToolProfile::Custom(vec!["read".to_string()])
        );
    }

    #[test]
    fn runtime_policy_from_cli_rejects_unknown_profiles() {
        let err = runtime_policy_from_cli("bogus", &HashMap::new(), &[], &[])
            .expect_err("unknown profile should fail");

        assert!(err.to_string().contains("unknown tool profile"));
    }
}
