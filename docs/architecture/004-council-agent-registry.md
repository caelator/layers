# RFC: Council Agent Registry

**Status:** Draft
**Date:** 2026-04-06
**Source:** Inspired by AoE's AgentDef registry (njbrake/agent-of-empires)

---

## Problem

Currently, council model invocations (`gemini_cmd`, `claude_cmd`, `codex_cmd`) are untyped strings configured in `layers.toml` or environment variables. There's no structured definition of:
- What each council participant can do
- How to detect if a binary is installed
- What permission/model constraints apply
- How to select the best model for a given stage

---

## Design

### AgentDef Struct

```rust
/// Represents a single council participant / model.
pub struct CouncilAgent {
    /// Canonical name: "gemini", "claude", "codex"
    pub name: &'static str,
    
    /// Binary to invoke (usually same as name, but "claude-code" vs "claude")
    pub binary: &'static str,
    
    /// Alternative binary names recognized
    pub aliases: &'static [&'static str],
    
    /// How to detect if this agent binary is available
    pub detection: DetectionMethod,
    
    /// What this agent is good at (for stage routing)
    pub capabilities: &'static [AgentCapability],
    
    /// Default model override (e.g., "claude-sonnet-4" for claude)
    pub default_model: Option<&'static str>,
    
    /// Whether this agent requires special permission flags
    pub permission_profile: PermissionProfile,
    
    /// Cost tier for routing decisions
    pub cost_tier: CostTier,
}

#[derive(Clone, Copy)]
pub enum DetectionMethod {
    /// Run `which <binary>` and check exit code
    Which(&'static str),
    /// Run `<binary> --version` and check no error
    RunWithArg(&'static str, &'static str),
    /// Always available (built-in or cloud API)
    AlwaysAvailable,
}

#[derive(Clone, Copy)]
pub enum AgentCapability {
    Reasoning,
    CodeGeneration,
    FastResponse,
    StructuredOutput,
    LongContext,
    MultiModal,
}

#[derive(Clone, Copy)]
pub enum PermissionProfile {
    /// Default permissions needed
    Standard,
    /// Needs --dangerously-skip-permissions or equivalent
    Elevated,
    /// No special permissions needed
    Unrestricted,
}

#[derive(Clone, Copy)]
pub enum CostTier {
    Low,
    Medium,
    High,
    Premium,
}
```

### Registry Definition

```rust
pub const COUNCIL_AGENTS: &[CouncilAgent] = &[
    CouncilAgent {
        name: "gemini",
        binary: "gemini",
        aliases: &["gemini-cli"],
        detection: DetectionMethod::Which("gemini"),
        capabilities: &[AgentCapability::Reasoning, AgentCapability::FastResponse],
        default_model: Some("gemini-2.5-flash"),
        permission_profile: PermissionProfile::Standard,
        cost_tier: CostTier::Low,
    },
    CouncilAgent {
        name: "claude",
        binary: "claude",
        aliases: &["claude-code"],
        detection: DetectionMethod::Which("claude"),
        capabilities: &[AgentCapability::Reasoning, AgentCapability::CodeGeneration, AgentCapability::StructuredOutput],
        default_model: Some("claude-sonnet-4"),
        permission_profile: PermissionProfile::Elevated,
        cost_tier: CostTier::Premium,
    },
    CouncilAgent {
        name: "codex",
        binary: "codex",
        aliases: &["opencode"],
        detection: DetectionMethod::Which("codex"),
        capabilities: &[AgentCapability::CodeGeneration, AgentCapability::LongContext],
        default_model: Some("codex"),
        permission_profile: PermissionProfile::Standard,
        cost_tier: CostTier::Medium,
    },
];
```

### ResolvedAgent

At runtime, agents are resolved to check availability:

```rust
pub struct ResolvedAgent {
    pub def: &'static CouncilAgent,
    pub available: bool,
    pub resolved_binary: Option<PathBuf>,  // path to the actual binary
    pub resolved_model: Option<String>,      // actual model to use
}

impl CouncilAgents {
    /// Resolve all known agents, checking availability on the host.
    pub fn resolve_all() -> Vec<ResolvedAgent> {
        COUNCIL_AGENTS.iter().map(|def| {
            let available = match def.detection {
                DetectionMethod::Which(binary) => {
                    std::process::Command::new("which")
                        .arg(binary)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false)
                }
                DetectionMethod::RunWithArg(binary, arg) => {
                    std::process::Command::new(binary)
                        .arg(arg)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false)
                }
                DetectionMethod::AlwaysAvailable => true,
            };
            
            ResolvedAgent {
                def,
                available,
                resolved_binary: available.then(|| 
                    which::which(def.binary).ok()
                ).flatten(),
                resolved_model: def.default_model.map(|m| m.to_string()),
            }
        }).collect()
    }
    
    /// List only available agents.
    pub fn available() -> Vec<ResolvedAgent> {
        Self::resolve_all().into_iter().filter(|a| a.available).collect()
    }
}
```

### CLI Integration

```bash
# Show all known council agents and their availability
layers council --list-agents

# Example output:
# NAME     BINARY      AVAILABLE  MODEL                COST
# gemini   gemini      ✓         gemini-2.5-flash    Low
# claude   claude      ✓         claude-sonnet-4     Premium  
# codex    codex       ✗         codex               Medium
```

### Stage Routing with Capabilities

The registry enables capability-based routing:

```rust
fn select_agent_for_stage(stage: &StageSpec, available: &[ResolvedAgent]) -> &ResolvedAgent {
    let required = &stage.required_capabilities;
    
    available
        .iter()
        .filter(|a| required.iter().all(|c| a.def.capabilities.contains(c)))
        .min_by_key(|a| a.def.cost_tier)  // prefer cheaper when capable
        .unwrap_or_else(|| available.first().expect("at least one agent available"))
}
```

---

## Integration with layers config

Currently in `layers.toml`:

```toml
[council]
gemini_cmd = "gemini"
claude_cmd = "claude"
codex_cmd = "codex"
```

With the registry, this becomes optional — the registry provides defaults and detection:

```toml
[council]
# Override detected defaults (optional)
# gemini_model = "gemini-2.5-pro"
# claude_model = "claude-opus-4"
```

---

## Out of Scope

- Dynamic agent registration at runtime (hard; defer)
- Agent capability self-reporting via API probing (interesting future work)
- Cost tracking / budget enforcement (belongs in a separate cost management layer)
