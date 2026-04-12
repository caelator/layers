use std::path::{Path, PathBuf};

use layers_core::config::LayersConfig;
use layers_store::config::ConfigStore;

/// XDG-style global config directory for Layers.
/// Uses `~/.config/layers/` on Unix, falls back to `~/.layers/`.
pub fn global_config_dir() -> PathBuf {
    dirs_home().join(".config").join("layers")
}

/// Path to the global `layers.toml` config file.
pub fn global_config_path() -> PathBuf {
    global_config_dir().join("layers.toml")
}

/// Load configuration with precedence:
///   1. CLI override path (if provided)
///   2. `LAYERS_WORKSPACE_ROOT/layers.toml` (workspace-local)
///   3. `~/.config/layers/layers.toml` (global)
///   4. Environment variable overrides (e.g. `LAYERS_DAEMON_PORT`)
///   5. Built-in defaults
///
/// Each layer is merged on top of the previous one.
pub fn load_config_with_precedence(cli_override: Option<&Path>) -> anyhow::Result<LayersConfig> {
    // Start with defaults
    let mut config = LayersConfig::default();

    // Layer 3: global config
    let global = global_config_path();
    if global.exists() {
        let store = ConfigStore::new(&global);
        config = merge_config(config, store.read().map_err(|e| anyhow::anyhow!("{e}"))?);
    }

    // Layer 2: workspace-local config
    let workspace = workspace_root().join("layers.toml");
    if workspace.exists() {
        let store = ConfigStore::new(&workspace);
        config = merge_config(config, store.read().map_err(|e| anyhow::anyhow!("{e}"))?);
    }

    // Layer 1: CLI override (highest priority file)
    if let Some(path) = cli_override {
        let store = ConfigStore::new(path);
        config = store.read().map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    // Layer 4: env var overrides
    apply_env_overrides(&mut config);

    Ok(config)
}

/// Merge `overlay` on top of `base`, replacing fields that are set in overlay.
fn merge_config(mut base: LayersConfig, overlay: LayersConfig) -> LayersConfig {
    // Daemon: overlay wins if non-default
    if overlay.daemon.port != 3000 {
        base.daemon.port = overlay.daemon.port;
    }
    if overlay.daemon.bind_address != "127.0.0.1" {
        base.daemon.bind_address = overlay.daemon.bind_address;
    }
    if overlay.daemon.tls.is_some() {
        base.daemon.tls = overlay.daemon.tls;
    }

    // Agent: overlay wins if set
    if overlay.agent.workspace.is_some() {
        base.agent.workspace = overlay.agent.workspace;
    }
    if overlay.agent.model.is_some() {
        base.agent.model = overlay.agent.model;
    }
    if overlay.agent.heartbeat_interval.is_some() {
        base.agent.heartbeat_interval = overlay.agent.heartbeat_interval;
    }
    if overlay.agent.timezone.is_some() {
        base.agent.timezone = overlay.agent.timezone;
    }
    if overlay.agent.context_window.is_some() {
        base.agent.context_window = overlay.agent.context_window;
    }
    if overlay.agent.max_context_tokens.is_some() {
        base.agent.max_context_tokens = overlay.agent.max_context_tokens;
    }
    if overlay.agent.compaction_threshold.is_some() {
        base.agent.compaction_threshold = overlay.agent.compaction_threshold;
    }

    // Providers, channels, agents: overlay entries replace base entries with same key
    for (k, v) in overlay.providers {
        base.providers.insert(k, v);
    }
    for (k, v) in overlay.channels {
        base.channels.insert(k, v);
    }
    for (k, v) in overlay.agents {
        base.agents.insert(k, v);
    }

    // Bindings: overlay replaces entirely if non-empty
    if !overlay.bindings.is_empty() {
        base.bindings = overlay.bindings;
    }

    // Tools: overlay replaces
    if !overlay.tools.allow.is_empty() {
        base.tools.allow = overlay.tools.allow;
    }
    if !overlay.tools.deny.is_empty() {
        base.tools.deny = overlay.tools.deny;
    }
    for (k, v) in overlay.tools.profiles {
        base.tools.profiles.insert(k, v);
    }

    // MCP: overlay servers replace
    for (k, v) in overlay.mcp.servers {
        base.mcp.servers.insert(k, v);
    }

    base
}

/// Apply environment variable overrides to the config.
fn apply_env_overrides(config: &mut LayersConfig) {
    if let Ok(port) = std::env::var("LAYERS_DAEMON_PORT") {
        if let Ok(p) = port.parse() {
            config.daemon.port = p;
        }
    }
    if let Ok(addr) = std::env::var("LAYERS_DAEMON_BIND") {
        config.daemon.bind_address = addr;
    }
    if let Ok(model) = std::env::var("LAYERS_AGENT_MODEL") {
        config.agent.model = Some(model);
    }
    if let Ok(tz) = std::env::var("LAYERS_AGENT_TIMEZONE") {
        config.agent.timezone = Some(tz);
    }
}

/// Mask sensitive values (API keys, tokens, secrets) in a config for display.
pub fn mask_secrets(config: &LayersConfig) -> LayersConfig {
    let mut c = config.clone();
    for provider in c.providers.values_mut() {
        if let Some(key) = provider.api_key.take() {
            provider.api_key = Some(mask_secret(&key));
        }
    }
    for channel in c.channels.values_mut() {
        if let Some(token) = channel.token.take() {
            channel.token = Some(mask_secret(&token));
        }
        if let Some(key) = channel.api_key.take() {
            channel.api_key = Some(mask_secret(&key));
        }
        if let Some(secret) = channel.webhook_secret.take() {
            channel.webhook_secret = Some(mask_secret(&secret));
        }
    }
    for server in c.mcp.servers.values_mut() {
        if let Some(key) = server.api_key.take() {
            server.api_key = Some(mask_secret(&key));
        }
    }
    c
}

/// Mask a single secret string, showing first 4 and last 4 chars if long enough.
fn mask_secret(s: &str) -> String {
    if s.len() <= 12 {
        return "****".to_string();
    }
    format!("{}****{}", &s[..4], &s[s.len() - 4..])
}

pub fn workspace_root() -> PathBuf {
    if let Ok(root) = std::env::var("LAYERS_WORKSPACE_ROOT") {
        return PathBuf::from(root);
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(git_root) = find_git_root(&cwd) {
            return git_root;
        }
        return cwd;
    }
    PathBuf::from(".")
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

pub fn memoryport_dir() -> PathBuf {
    workspace_root().join("memoryport")
}

pub fn canonical_curated_memory_path() -> PathBuf {
    memoryport_dir().join("curated-memory.jsonl")
}

pub fn uc_config_path() -> PathBuf {
    dirs_home().join(".memoryport").join("uc.toml")
}

pub fn council_files() -> Vec<(&'static str, PathBuf)> {
    let base = memoryport_dir();
    vec![
        ("plan", base.join("council-plans.jsonl")),
        ("trace", base.join("council-traces.jsonl")),
        ("learning", base.join("council-learnings.jsonl")),
    ]
}

/// Timeout in milliseconds before falling back from `uc` to local JSONL.
/// Override with `LAYERS_UC_TIMEOUT_MS`.
pub fn uc_timeout_ms() -> u64 {
    std::env::var("LAYERS_UC_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500)
}

/// Minimum results from `uc` to consider the retrieval successful.
/// If fewer are returned, local JSONL gets boosted.
/// Override with `LAYERS_UC_MIN_RESULTS`.
pub fn uc_min_results() -> usize {
    std::env::var("LAYERS_UC_MIN_RESULTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

/// Current schema version for `ContextPayload`.
pub const CONTEXT_PAYLOAD_SCHEMA_VERSION: u32 = 2;

fn dirs_home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
}
