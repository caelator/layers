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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use layers_core::{ChannelHealth, OutboundMessage, StreamingTarget};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Mock adapter for testing the manager's routing and registration logic.
    struct MockAdapter {
        adapter_name: String,
        send_count: AtomicUsize,
        streaming_count: AtomicUsize,
        reaction_count: AtomicUsize,
    }

    impl MockAdapter {
        fn new(name: &str) -> Self {
            Self {
                adapter_name: name.to_string(),
                send_count: AtomicUsize::new(0),
                streaming_count: AtomicUsize::new(0),
                reaction_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ChannelAdapter for MockAdapter {
        fn name(&self) -> &str {
            &self.adapter_name
        }

        async fn start(&self, _cancel: CancellationToken) -> Result<()> {
            Ok(())
        }

        async fn stop(&self) -> Result<()> {
            Ok(())
        }

        async fn send(&self, _message: OutboundMessage) -> Result<()> {
            self.send_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn send_streaming(&self, _target: StreamingTarget, _chunk: String) -> Result<()> {
            self.streaming_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn send_reaction(
            &self,
            _channel: &str,
            _message_id: &str,
            _emoji: &str,
        ) -> Result<()> {
            self.reaction_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn health(&self) -> ChannelHealth {
            ChannelHealth::Connected
        }
    }

    fn make_inbound(channel: &str, msg_id: &str) -> InboundMessage {
        InboundMessage {
            channel: channel.to_string(),
            channel_message_id: msg_id.to_string(),
            peer_id: "user1".to_string(),
            peer_display_name: "Test User".to_string(),
            peer_kind: layers_core::PeerKind::User,
            text: "hello".to_string(),
            attachments: Vec::new(),
            thread_id: None,
            reply_to_message_id: None,
            channel_metadata: None,
            timestamp: chrono::Utc::now(),
        }
    }

    fn make_outbound(channel: &str) -> OutboundMessage {
        OutboundMessage {
            channel: channel.to_string(),
            text: "reply".to_string(),
            thread_id: None,
            reply_to_message_id: None,
            attachments: Vec::new(),
            streaming: None,
        }
    }

    #[tokio::test]
    async fn register_and_adapter_count() {
        let (mgr, _rx) = ChannelManager::new(16, 500);
        assert_eq!(mgr.adapter_count().await, 0);

        mgr.register(Arc::new(MockAdapter::new("alpha"))).await;
        assert_eq!(mgr.adapter_count().await, 1);

        mgr.register(Arc::new(MockAdapter::new("beta"))).await;
        assert_eq!(mgr.adapter_count().await, 2);
    }

    #[tokio::test]
    async fn submit_inbound_dedup_drops_duplicate() {
        let (mgr, mut rx) = ChannelManager::new(16, 5_000);

        let msg1 = make_inbound("test-ch", "msg-42");
        let msg2 = make_inbound("test-ch", "msg-42"); // same channel + id

        mgr.submit_inbound(msg1).await.unwrap();
        mgr.submit_inbound(msg2).await.unwrap(); // should be silently dropped

        // Only one message should have been delivered.
        let received = rx.try_recv();
        assert!(received.is_ok());
        let second = rx.try_recv();
        assert!(second.is_err()); // nothing else
    }

    #[tokio::test]
    async fn dispatch_outbound_routes_to_correct_adapter() {
        let (mgr, _rx) = ChannelManager::new(16, 500);

        let adapter_a = Arc::new(MockAdapter::new("chan-a"));
        let adapter_b = Arc::new(MockAdapter::new("chan-b"));

        mgr.register(adapter_a.clone()).await;
        mgr.register(adapter_b.clone()).await;

        // Send to chan-a
        mgr.dispatch_outbound(make_outbound("chan-a")).await.unwrap();
        assert_eq!(adapter_a.send_count.load(Ordering::SeqCst), 1);
        assert_eq!(adapter_b.send_count.load(Ordering::SeqCst), 0);

        // Send to chan-b
        mgr.dispatch_outbound(make_outbound("chan-b")).await.unwrap();
        assert_eq!(adapter_b.send_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_outbound_unknown_channel_returns_error() {
        let (mgr, _rx) = ChannelManager::new(16, 500);

        let result = mgr.dispatch_outbound(make_outbound("nonexistent")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn health_all_returns_registered_adapters() {
        let (mgr, _rx) = ChannelManager::new(16, 500);

        mgr.register(Arc::new(MockAdapter::new("foo"))).await;
        mgr.register(Arc::new(MockAdapter::new("bar"))).await;

        let handles = mgr.health_all().await;
        assert_eq!(handles.len(), 2);

        let names: Vec<&str> = handles.iter().map(|h| h.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"bar"));

        // MockAdapter always returns Connected.
        for h in &handles {
            assert_eq!(h.health, ChannelHealth::Connected);
        }
    }

    #[tokio::test]
    async fn model_override_resolution() {
        let (mgr, _rx) = ChannelManager::new(16, 500);

        let mut account_overrides = std::collections::HashMap::new();
        account_overrides.insert("vip-user".to_string(), "openai:gpt-4o".to_string());

        mgr.set_model_overrides(vec![crate::types::ChannelModelOverride {
            channel: "telegram".to_string(),
            model: Some("anthropic:claude-3".to_string()),
            account_overrides,
        }])
        .await;

        // Channel-level default
        let resolved = mgr.resolve_model_override("telegram", None).await;
        assert_eq!(resolved, Some("anthropic:claude-3".to_string()));

        // Per-peer override takes precedence
        let resolved = mgr
            .resolve_model_override("telegram", Some("vip-user"))
            .await;
        assert_eq!(resolved, Some("openai:gpt-4o".to_string()));

        // Unknown peer falls back to channel default
        let resolved = mgr
            .resolve_model_override("telegram", Some("regular-user"))
            .await;
        assert_eq!(resolved, Some("anthropic:claude-3".to_string()));

        // Unknown channel returns None
        let resolved = mgr.resolve_model_override("unknown", None).await;
        assert_eq!(resolved, None);
    }

    #[tokio::test]
    async fn dispatch_streaming_routes_correctly() {
        let (mgr, _rx) = ChannelManager::new(16, 500);

        let adapter = Arc::new(MockAdapter::new("stream-ch"));
        mgr.register(adapter.clone()).await;

        let target = StreamingTarget {
            channel: "stream-ch".to_string(),
            thread_id: None,
            message_id: None,
        };

        mgr.dispatch_streaming(target, "chunk1".to_string())
            .await
            .unwrap();
        assert_eq!(adapter.streaming_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unregister_removes_adapter() {
        let (mgr, _rx) = ChannelManager::new(16, 500);

        mgr.register(Arc::new(MockAdapter::new("temp"))).await;
        assert_eq!(mgr.adapter_count().await, 1);

        let removed = mgr.unregister("temp").await;
        assert!(removed.is_some());
        assert_eq!(mgr.adapter_count().await, 0);
    }
}
