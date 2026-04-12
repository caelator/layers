//! Channel manager: registry, routing, health monitoring, dedup/debounce.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use layers_core::{
    CancellationToken, ChannelAdapter, ChannelHealth, ChannelRuntimeHandle, InboundMessage,
    OutboundMessage, Result, StreamingTarget,
};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{info, warn};

use crate::types::{ChannelModelOverride, DedupKey};

/// Central registry and router for all channel adapters.
pub struct ChannelManager {
    adapters: RwLock<HashMap<String, Arc<dyn ChannelAdapter>>>,
    inbound_tx: mpsc::Sender<InboundMessage>,
    model_overrides: RwLock<Vec<ChannelModelOverride>>,
    recent_ids: Arc<Mutex<HashSet<DedupKey>>>,
    debounce_ms: u64,
}

impl ChannelManager {
    /// Create a new channel manager. Returns the manager and a receiver for inbound messages.
    #[must_use]
    pub fn new(buffer: usize, debounce_ms: u64) -> (Self, mpsc::Receiver<InboundMessage>) {
        let (tx, rx) = mpsc::channel(buffer);
        let mgr = Self {
            adapters: RwLock::new(HashMap::new()),
            inbound_tx: tx,
            model_overrides: RwLock::new(Vec::new()),
            recent_ids: Arc::new(Mutex::new(HashSet::new())),
            debounce_ms,
        };
        (mgr, rx)
    }

    /// Register a channel adapter.
    pub async fn register(&self, adapter: Arc<dyn ChannelAdapter>) {
        let name = adapter.name().to_string();
        info!(channel = %name, "registering channel adapter");
        self.adapters.write().await.insert(name, adapter);
    }

    /// Unregister a channel adapter by name.
    pub async fn unregister(&self, name: &str) -> Option<Arc<dyn ChannelAdapter>> {
        self.adapters.write().await.remove(name)
    }

    /// Start all registered adapters.
    ///
    /// # Errors
    /// Returns an error if any adapter fails to start.
    pub async fn start_all(&self, cancel: CancellationToken) -> Result<()> {
        let adapters = self.adapters.read().await;
        for (name, adapter) in &*adapters {
            info!(channel = %name, "starting channel adapter");
            adapter.start(cancel.clone()).await?;
        }
        Ok(())
    }

    /// Stop all registered adapters.
    ///
    /// # Errors
    /// Returns an error if any adapter fails to stop.
    pub async fn stop_all(&self) -> Result<()> {
        let adapters = self.adapters.read().await;
        for (name, adapter) in &*adapters {
            info!(channel = %name, "stopping channel adapter");
            adapter.stop().await?;
        }
        Ok(())
    }

    /// Submit an inbound message with dedup and debounce.
    ///
    /// # Errors
    /// Returns an error if the inbound channel is closed.
    pub async fn submit_inbound(&self, message: InboundMessage) -> Result<()> {
        let key = DedupKey::from_inbound(&message);

        // Dedup check
        {
            let mut recent = self.recent_ids.lock().await;
            if !recent.insert(key.clone()) {
                warn!(
                    channel = %message.channel,
                    msg_id = %message.channel_message_id,
                    "dropping duplicate inbound message"
                );
                return Ok(());
            }
        }

        // Schedule cleanup of dedup entry after debounce window
        if self.debounce_ms > 0 {
            let recent = self.recent_ids.clone();
            let debounce = self.debounce_ms;
            let cleanup_key = key;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(debounce)).await;
                recent.lock().await.remove(&cleanup_key);
            });
        }

        self.inbound_tx
            .send(message)
            .await
            .map_err(|_| layers_core::LayersError::Channel("inbound channel closed".into()))
    }

    /// Dispatch an outbound message to the correct adapter.
    ///
    /// # Errors
    /// Returns an error if the adapter is not found or fails to send.
    pub async fn dispatch_outbound(&self, message: OutboundMessage) -> Result<()> {
        let adapters = self.adapters.read().await;
        let adapter = adapters.get(&message.channel).ok_or_else(|| {
            layers_core::LayersError::Channel(format!(
                "no adapter registered for channel '{}'",
                message.channel
            ))
        })?;
        adapter.send(message).await
    }

    /// Dispatch a streaming chunk to the correct adapter.
    ///
    /// # Errors
    /// Returns an error if the adapter is not found or fails to send.
    pub async fn dispatch_streaming(&self, target: StreamingTarget, chunk: String) -> Result<()> {
        let adapters = self.adapters.read().await;
        let adapter = adapters.get(&target.channel).ok_or_else(|| {
            layers_core::LayersError::Channel(format!(
                "no adapter registered for channel '{}'",
                target.channel
            ))
        })?;
        adapter.send_streaming(target, chunk).await
    }

    /// Get health status for all adapters.
    pub async fn health_all(&self) -> Vec<ChannelRuntimeHandle> {
        let adapters = self.adapters.read().await;
        let mut handles = Vec::with_capacity(adapters.len());
        for (name, adapter) in &*adapters {
            let health = adapter.health().await;
            // Create a dummy sender — the handle is used for status reporting only here.
            let (tx, _rx) = mpsc::channel(1);
            handles.push(ChannelRuntimeHandle {
                name: name.clone(),
                outbound: tx,
                health,
            });
        }
        handles
    }

    /// Get health for a single adapter.
    pub async fn health_of(&self, name: &str) -> Option<ChannelHealth> {
        let adapters = self.adapters.read().await;
        let adapter = adapters.get(name)?;
        Some(adapter.health().await)
    }

    /// Set model overrides.
    pub async fn set_model_overrides(&self, overrides: Vec<ChannelModelOverride>) {
        *self.model_overrides.write().await = overrides;
    }

    /// Look up a model override for a given channel and optional peer.
    pub async fn resolve_model_override(
        &self,
        channel: &str,
        peer_id: Option<&str>,
    ) -> Option<String> {
        let overrides = self.model_overrides.read().await;
        for ov in &*overrides {
            if ov.channel == channel {
                if let Some(pid) = peer_id {
                    if let Some(model) = ov.account_overrides.get(pid) {
                        return Some(model.clone());
                    }
                }
                if let Some(ref model) = ov.model {
                    return Some(model.clone());
                }
            }
        }
        None
    }

    /// Number of registered adapters.
    pub async fn adapter_count(&self) -> usize {
        self.adapters.read().await.len()
    }

    /// Get a clone of the inbound sender for adapters to use.
    #[must_use]
    pub fn inbound_sender(&self) -> mpsc::Sender<InboundMessage> {
        self.inbound_tx.clone()
    }
}

// `Mutex` is held briefly and only across awaits for spawn cleanup — safe to send across threads.
unsafe impl Sync for ChannelManager {}
