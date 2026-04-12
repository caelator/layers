//! WebChat adapter — WebSocket-based webchat channel.
//!
//! Designed to mount into the axum gateway. The adapter holds an inbound sender
//! and manages connected WebSocket clients for outbound delivery.

use std::collections::HashMap;

use async_trait::async_trait;
use layers_core::{
    CancellationToken, ChannelAdapter, ChannelHealth, InboundMessage, LayersError,
    OutboundMessage, PeerKind, Result, StreamingTarget,
};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{info, warn};

/// A connected WebSocket client session.
struct WsClient {
    peer_id: String,
    sender: mpsc::Sender<String>,
}

/// `WebChatAdapter` serves as the channel adapter for browser-based webchat
/// connections via WebSocket.
pub struct WebChatAdapter {
    inbound_tx: mpsc::Sender<InboundMessage>,
    clients: RwLock<HashMap<String, WsClient>>,
    health: Mutex<ChannelHealth>,
    cancel: Mutex<Option<CancellationToken>>,
}

impl WebChatAdapter {
    /// Create a new `WebChatAdapter` with the given inbound message sender.
    #[must_use]
    pub fn new(inbound_tx: mpsc::Sender<InboundMessage>) -> Self {
        Self {
            inbound_tx,
            clients: RwLock::new(HashMap::new()),
            health: Mutex::new(ChannelHealth::Disconnected),
            cancel: Mutex::new(None),
        }
    }

    /// Register a new WebSocket client. Returns a receiver for outbound messages to that client.
    pub async fn register_client(
        &self,
        client_id: String,
        peer_id: String,
    ) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel(64);
        self.clients.write().await.insert(
            client_id,
            WsClient {
                peer_id,
                sender: tx,
            },
        );
        rx
    }

    /// Remove a disconnected WebSocket client.
    pub async fn remove_client(&self, client_id: &str) {
        self.clients.write().await.remove(client_id);
    }

    /// Handle an inbound text message from a connected WebSocket client.
    ///
    /// # Errors
    /// Returns an error if the inbound channel is closed.
    pub async fn handle_ws_message(
        &self,
        client_id: &str,
        peer_display_name: &str,
        text: String,
    ) -> Result<()> {
        let clients = self.clients.read().await;
        let client = clients.get(client_id).ok_or_else(|| {
            LayersError::Channel(format!("unknown webchat client '{client_id}'"))
        })?;

        let msg = InboundMessage {
            channel: "webchat".to_string(),
            channel_message_id: uuid::Uuid::new_v4().to_string(),
            peer_id: client.peer_id.clone(),
            peer_display_name: peer_display_name.to_string(),
            peer_kind: PeerKind::User,
            text,
            attachments: Vec::new(),
            thread_id: None,
            reply_to_message_id: None,
            channel_metadata: None,
            timestamp: chrono::Utc::now(),
        };

        self.inbound_tx
            .send(msg)
            .await
            .map_err(|_| LayersError::Channel("inbound channel closed".into()))
    }

    /// Count of currently connected clients.
    pub async fn client_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Send a raw string to a specific client.
    #[allow(dead_code)]
    async fn send_to_client(&self, peer_id: &str, text: &str) -> Result<()> {
        let clients = self.clients.read().await;
        for client in clients.values() {
            if client.peer_id == peer_id {
                client
                    .sender
                    .send(text.to_string())
                    .await
                    .map_err(|_| {
                        LayersError::Channel(format!(
                            "failed to send to webchat client '{peer_id}'"
                        ))
                    })?;
                return Ok(());
            }
        }
        warn!(peer_id = %peer_id, "webchat client not connected");
        Ok(())
    }

    /// Broadcast a string to all connected clients.
    async fn broadcast(&self, text: &str) {
        let clients = self.clients.read().await;
        for client in clients.values() {
            let _ = client.sender.send(text.to_string()).await;
        }
    }
}

#[async_trait]
impl ChannelAdapter for WebChatAdapter {
    fn name(&self) -> &str {
        "webchat"
    }

    async fn start(&self, cancel: CancellationToken) -> Result<()> {
        info!("webchat adapter started");
        *self.cancel.lock().await = Some(cancel);
        *self.health.lock().await = ChannelHealth::Connected;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("webchat adapter stopping");
        if let Some(cancel) = self.cancel.lock().await.take() {
            cancel.cancel();
        }
        *self.health.lock().await = ChannelHealth::Disconnected;
        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> Result<()> {
        let payload = serde_json::json!({
            "type": "message",
            "text": message.text,
            "thread_id": message.thread_id,
            "attachments": message.attachments.iter().map(|a| {
                serde_json::json!({
                    "url": a.url,
                    "mime_type": a.mime_type,
                    "filename": a.filename,
                })
            }).collect::<Vec<_>>(),
        });
        let text = serde_json::to_string(&payload)
            .map_err(|e| LayersError::Channel(format!("serialization error: {e}")))?;

        // Broadcast to all connected clients on the webchat channel.
        self.broadcast(&text).await;
        Ok(())
    }

    async fn send_streaming(&self, _target: StreamingTarget, chunk: String) -> Result<()> {
        let payload = serde_json::json!({
            "type": "stream_chunk",
            "text": chunk,
        });
        let text = serde_json::to_string(&payload)
            .map_err(|e| LayersError::Channel(format!("serialization error: {e}")))?;
        self.broadcast(&text).await;
        Ok(())
    }

    async fn send_reaction(&self, _channel: &str, _message_id: &str, emoji: &str) -> Result<()> {
        let payload = serde_json::json!({
            "type": "reaction",
            "emoji": emoji,
        });
        let text = serde_json::to_string(&payload)
            .map_err(|e| LayersError::Channel(format!("serialization error: {e}")))?;
        self.broadcast(&text).await;
        Ok(())
    }

    async fn health(&self) -> ChannelHealth {
        self.health.lock().await.clone()
    }
}
