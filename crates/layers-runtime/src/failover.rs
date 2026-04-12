//! Model failover: try primary → fallback chain with cooldown tracking.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tracing::{info, warn};

use layers_core::{
    LayersError, ModelRef, ModelRequest, ModelResponse, Result,
};
use layers_providers::registry::ProviderRegistry;

use crate::agent_loop::is_failover_worthy;

// ---------------------------------------------------------------------------
// Cooldown tracker
// ---------------------------------------------------------------------------

/// Tracks cooldown state for auth profiles / provider keys.
#[derive(Debug, Clone)]
struct CooldownEntry {
    /// When the cooldown expires.
    until: Instant,
    /// Current backoff duration (doubles each time).
    backoff: Duration,
}

/// Manages per-profile cooldowns with exponential backoff.
pub struct CooldownTracker {
    entries: RwLock<HashMap<String, CooldownEntry>>,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl CooldownTracker {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            initial_backoff: Duration::from_secs(5),
            max_backoff: Duration::from_secs(300),
        }
    }

    pub fn with_backoff(initial: Duration, max: Duration) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            initial_backoff: initial,
            max_backoff: max,
        }
    }

    /// Check if a provider/profile is currently in cooldown.
    pub async fn is_cooled_down(&self, profile: &str) -> bool {
        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(profile) {
            Instant::now() < entry.until
        } else {
            false
        }
    }

    /// Record a failure for a provider/profile, starting or extending cooldown.
    pub async fn record_failure(&self, profile: &str) {
        let mut entries = self.entries.write().await;
        let entry = entries.entry(profile.to_string()).or_insert(CooldownEntry {
            until: Instant::now(),
            backoff: self.initial_backoff,
        });

        // Exponential backoff.
        entry.until = Instant::now() + entry.backoff;
        entry.backoff = (entry.backoff * 2).min(self.max_backoff);
    }

    /// Clear cooldown for a provider/profile (e.g., on successful request).
    pub async fn clear(&self, profile: &str) {
        self.entries.write().await.remove(profile);
    }
}

impl Default for CooldownTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Failover chain
// ---------------------------------------------------------------------------

/// Manages model failover: primary → fallback chain with cooldown awareness.
pub struct FailoverChain {
    registry: Arc<ProviderRegistry>,
    /// Ordered list of fallback model refs (tried in order after primary fails).
    fallbacks: Vec<ModelRef>,
    cooldowns: CooldownTracker,
}

impl FailoverChain {
    pub fn new(registry: Arc<ProviderRegistry>, fallbacks: Vec<ModelRef>) -> Self {
        Self {
            registry,
            fallbacks,
            cooldowns: CooldownTracker::new(),
        }
    }

    pub fn with_cooldowns(mut self, tracker: CooldownTracker) -> Self {
        self.cooldowns = tracker;
        self
    }

    /// Attempt failover after the primary model failed with `primary_err`.
    /// Tries each fallback in order, skipping those in cooldown.
    pub async fn try_failover(
        &self,
        mut request: ModelRequest,
        primary_err: &LayersError,
    ) -> Result<ModelResponse> {
        if !is_failover_worthy(primary_err) {
            return Err(LayersError::Provider(format!(
                "error is not failover-worthy: {primary_err}"
            )));
        }

        // Record cooldown for the primary that just failed.
        let primary_key = request.model.full_id();
        self.cooldowns.record_failure(&primary_key).await;
        warn!(primary = %primary_key, "primary model failed, attempting failover");

        for fallback in &self.fallbacks {
            let fb_key = fallback.full_id();

            // Skip if in cooldown.
            if self.cooldowns.is_cooled_down(&fb_key).await {
                info!(fallback = %fb_key, "skipping — in cooldown");
                continue;
            }

            // Resolve provider.
            let provider = match self.registry.resolve(fallback) {
                Some(p) => p,
                None => {
                    warn!(fallback = %fb_key, "fallback provider not found in registry");
                    continue;
                }
            };

            // Update request model.
            request.model = fallback.clone();

            match provider.complete(request.clone()).await {
                Ok(response) => {
                    info!(fallback = %fb_key, "failover succeeded");
                    self.cooldowns.clear(&fb_key).await;
                    return Ok(response);
                }
                Err(e) if is_failover_worthy(&e) => {
                    warn!(fallback = %fb_key, error = %e, "fallback also failed, trying next");
                    self.cooldowns.record_failure(&fb_key).await;
                    continue;
                }
                Err(e) => {
                    // Non-failover-worthy error — propagate immediately.
                    return Err(e);
                }
            }
        }

        Err(LayersError::FallbackExhausted)
    }
}
