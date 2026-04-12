//! Lifecycle hooks — pre/post agent run and interception hooks.
//!
//! Hooks allow injecting behavior around agent runs, with support for
//! debounce and deduplication of interception hooks.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use layers_core::{LayersError, Result, Session};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// When the hook fires relative to the agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPhase {
    PreRun,
    PostRun,
    Intercept,
}

/// Configuration for a single hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    pub name: String,
    pub phase: HookPhase,
    pub command: String,
    /// Minimum time between firings for interception hooks.
    pub debounce_ms: Option<u64>,
    /// Deduplicate by this key — only fire once per unique key.
    pub dedup_key: Option<String>,
    pub enabled: bool,
}

/// Result of executing a hook.
#[derive(Debug, Clone)]
pub struct HookResult {
    pub hook_name: String,
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration: Duration,
}

/// Manages lifecycle hooks for agent runs.
pub struct HookManager {
    hooks: RwLock<Vec<HookConfig>>,
    last_fired: RwLock<HashMap<String, Instant>>,
    dedup_seen: RwLock<HashMap<String, String>>,
}

impl HookManager {
    /// Create a new hook manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hooks: RwLock::new(Vec::new()),
            last_fired: RwLock::new(HashMap::new()),
            dedup_seen: RwLock::new(HashMap::new()),
        }
    }

    /// Load hooks from config.
    pub async fn load_hooks(&self, configs: Vec<HookConfig>) {
        let count = configs.len();
        *self.hooks.write().await = configs;
        info!(count, "hooks loaded");
    }

    /// Register a single hook.
    pub async fn register(&self, config: HookConfig) {
        info!(name = %config.name, phase = ?config.phase, "registering hook");
        self.hooks.write().await.push(config);
    }

    /// Fire all hooks matching the given phase.
    ///
    /// # Errors
    /// Returns an error if a pre-run hook fails (post-run and intercept failures are logged but not propagated).
    pub async fn fire(
        &self,
        phase: HookPhase,
        session: &Session,
    ) -> Result<Vec<HookResult>> {
        let hooks = self.hooks.read().await.clone();
        let matching: Vec<_> = hooks
            .iter()
            .filter(|h| h.enabled && h.phase == phase)
            .collect();

        let mut results = Vec::new();
        for hook in matching {
            // Debounce check.
            if let Some(debounce_ms) = hook.debounce_ms {
                let last = self.last_fired.read().await;
                if let Some(last_time) = last.get(&hook.name) {
                    if last_time.elapsed() < Duration::from_millis(debounce_ms) {
                        debug!(hook = %hook.name, "debounced — skipping");
                        continue;
                    }
                }
            }

            // Dedup check.
            if let Some(ref dedup_key) = hook.dedup_key {
                let key = format!("{}:{dedup_key}", session.id);
                let seen = self.dedup_seen.read().await;
                if seen.contains_key(&key) {
                    debug!(hook = %hook.name, key = %key, "deduplicated — skipping");
                    continue;
                }
                drop(seen);
                self.dedup_seen
                    .write()
                    .await
                    .insert(key, hook.name.clone());
            }

            let result = execute_hook(hook, session).await;
            self.last_fired
                .write()
                .await
                .insert(hook.name.clone(), Instant::now());

            if phase == HookPhase::PreRun && !result.success {
                warn!(hook = %hook.name, "pre-run hook failed — aborting run");
                results.push(result);
                return Err(LayersError::Channel(format!(
                    "pre-run hook '{}' failed",
                    hook.name
                )));
            }

            results.push(result);
        }

        Ok(results)
    }

    /// Clear dedup state (e.g. between sessions).
    pub async fn clear_dedup(&self) {
        self.dedup_seen.write().await.clear();
    }

    /// Number of registered hooks.
    pub async fn hook_count(&self) -> usize {
        self.hooks.read().await.len()
    }
}

impl Default for HookManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute a single hook command.
async fn execute_hook(hook: &HookConfig, session: &Session) -> HookResult {
    let start = Instant::now();
    info!(hook = %hook.name, session_id = %session.id, "executing hook");

    // Execute the command via tokio subprocess.
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&hook.command)
        .env("LAYERS_SESSION_ID", &session.id)
        .env("LAYERS_AGENT_ID", &session.agent_id)
        .env("LAYERS_HOOK_PHASE", format!("{:?}", hook.phase))
        .output()
        .await;

    let duration = start.elapsed();

    match output {
        Ok(out) => {
            let success = out.status.success();
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();

            if !success {
                warn!(
                    hook = %hook.name,
                    stderr = %stderr,
                    "hook exited with non-zero status"
                );
            }

            HookResult {
                hook_name: hook.name.clone(),
                success,
                output: if stdout.is_empty() {
                    None
                } else {
                    Some(stdout)
                },
                error: if stderr.is_empty() {
                    None
                } else {
                    Some(stderr)
                },
                duration,
            }
        }
        Err(e) => {
            warn!(hook = %hook.name, error = %e, "hook execution failed");
            HookResult {
                hook_name: hook.name.clone(),
                success: false,
                output: None,
                error: Some(e.to_string()),
                duration,
            }
        }
    }
}
