//! Telegram adapter — HTTP polling via reqwest (no teloxide dependency).
//!
//! Uses the Telegram Bot API with long-polling `getUpdates` to receive
//! messages and REST endpoints to send replies, edit streaming messages,
//! and set reactions.

use async_trait::async_trait;
use layers_core::{
    CancellationToken, ChannelAdapter, ChannelHealth, InboundMessage, LayersError, OutboundMessage,
    PeerKind, Result, StreamingTarget,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Telegram API types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TgResponse<T> {
    ok: bool,
    #[serde(default)]
    description: Option<String>,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct TgUpdate {
    update_id: i64,
    message: Option<TgMessage>,
}

#[derive(Debug, Deserialize)]
struct TgMessage {
    message_id: i64,
    from: Option<TgUser>,
    #[allow(dead_code)]
    chat: TgChat,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgUser {
    id: i64,
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: Option<String>,
    #[serde(default)]
    is_bot: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TgChat {
    id: i64,
}

#[derive(Debug, Serialize)]
struct SendMessageRequest {
    chat_id: i64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SendMessageResult {
    message_id: i64,
}

#[derive(Debug, Serialize)]
struct EditMessageTextRequest {
    chat_id: i64,
    message_id: i64,
    text: String,
}

#[derive(Debug, Serialize)]
struct SetReactionRequest {
    chat_id: i64,
    message_id: i64,
    reaction: Vec<ReactionType>,
}

#[derive(Debug, Serialize)]
struct ReactionType {
    #[serde(rename = "type")]
    kind: String,
    emoji: String,
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// Telegram bot adapter using HTTP long-polling.
pub struct TelegramAdapter {
    bot_token: String,
    client: Client,
    inbound_tx: mpsc::Sender<InboundMessage>,
    polling_active: AtomicBool,
    cancel: Mutex<Option<CancellationToken>>,
    /// Tracks streaming message state: (chat_id, message_id, accumulated_text).
    streaming_state: Mutex<Option<(i64, i64, String)>>,
}

impl TelegramAdapter {
    /// Create a new Telegram adapter with the given bot token and inbound sender.
    #[must_use]
    pub fn new(bot_token: String, inbound_tx: mpsc::Sender<InboundMessage>) -> Self {
        Self {
            bot_token,
            client: Client::new(),
            inbound_tx,
            polling_active: AtomicBool::new(false),
            cancel: Mutex::new(None),
            streaming_state: Mutex::new(None),
        }
    }

    /// Returns the configured bot token.
    #[must_use]
    pub fn bot_token(&self) -> &str {
        &self.bot_token
    }

    /// Build a Telegram Bot API URL for the given method.
    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.bot_token)
    }

    /// Run the long-polling loop: calls getUpdates every ~2 s.
    async fn poll_loop(
        client: Client,
        api_base: String,
        inbound_tx: mpsc::Sender<InboundMessage>,
        cancel: CancellationToken,
    ) {
        let mut offset: Option<i64> = None;

        loop {
            if cancel.is_cancelled() {
                break;
            }

            let mut params = vec![("timeout", "2".to_string())];
            if let Some(off) = offset {
                params.push(("offset", off.to_string()));
            }

            let url = format!("{api_base}/getUpdates");
            let result = tokio::select! {
                () = cancel.cancelled() => break,
                res = client.get(&url).query(&params).send() => res,
            };

            match result {
                Ok(resp) => {
                    let body = match resp.json::<TgResponse<Vec<TgUpdate>>>().await {
                        Ok(b) => b,
                        Err(e) => {
                            warn!(error = %e, "failed to parse getUpdates response");
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            continue;
                        }
                    };

                    if !body.ok {
                        warn!(desc = ?body.description, "getUpdates returned ok=false");
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }

                    if let Some(updates) = body.result {
                        for update in updates {
                            offset = Some(update.update_id + 1);

                            if let Some(msg) = update.message {
                                let text = match msg.text {
                                    Some(t) if !t.is_empty() => t,
                                    _ => continue,
                                };

                                let (peer_id, peer_name, peer_kind) = match msg.from {
                                    Some(user) => {
                                        let name = match user.last_name {
                                            Some(ref ln) => {
                                                format!("{} {ln}", user.first_name)
                                            }
                                            None => user.first_name.clone(),
                                        };
                                        let kind = if user.is_bot {
                                            PeerKind::Bot
                                        } else {
                                            PeerKind::User
                                        };
                                        (user.id.to_string(), name, kind)
                                    }
                                    None => (
                                        "unknown".to_string(),
                                        "Unknown".to_string(),
                                        PeerKind::User,
                                    ),
                                };

                                let inbound = InboundMessage {
                                    channel: "telegram".to_string(),
                                    channel_message_id: msg.message_id.to_string(),
                                    peer_id,
                                    peer_display_name: peer_name,
                                    peer_kind,
                                    text,
                                    attachments: Vec::new(),
                                    thread_id: None,
                                    reply_to_message_id: None,
                                    channel_metadata: None,
                                    timestamp: chrono::Utc::now(),
                                };

                                if inbound_tx.send(inbound).await.is_err() {
                                    debug!("inbound channel closed — stopping poll loop");
                                    return;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "getUpdates HTTP request failed");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }
}

#[async_trait]
impl ChannelAdapter for TelegramAdapter {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&self, cancel: CancellationToken) -> Result<()> {
        info!("telegram adapter starting — polling getUpdates");
        self.polling_active.store(true, Ordering::SeqCst);
        *self.cancel.lock().await = Some(cancel.clone());

        let client = self.client.clone();
        let api_base = format!("https://api.telegram.org/bot{}", self.bot_token);
        let inbound_tx = self.inbound_tx.clone();

        tokio::spawn(async move {
            Self::poll_loop(client, api_base, inbound_tx, cancel).await;
        });

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("telegram adapter stopping");
        if let Some(cancel) = self.cancel.lock().await.take() {
            cancel.cancel();
        }
        self.polling_active.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> Result<()> {
        // The channel field carries the chat_id for Telegram.
        let chat_id: i64 = message.channel.parse().map_err(|_| {
            LayersError::Channel(
                "telegram send requires a numeric chat_id in OutboundMessage.channel".into(),
            )
        })?;

        let reply_to = message
            .reply_to_message_id
            .as_ref()
            .and_then(|id| id.parse::<i64>().ok());

        let req = SendMessageRequest {
            chat_id,
            text: message.text,
            reply_to_message_id: reply_to,
        };

        let resp = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&req)
            .send()
            .await
            .map_err(|e| LayersError::Channel(format!("telegram sendMessage failed: {e}")))?;

        let body: TgResponse<SendMessageResult> = resp
            .json()
            .await
            .map_err(|e| LayersError::Channel(format!("telegram sendMessage parse error: {e}")))?;

        if !body.ok {
            return Err(LayersError::Channel(format!(
                "telegram sendMessage error: {}",
                body.description.unwrap_or_default()
            )));
        }

        Ok(())
    }

    async fn send_streaming(&self, target: StreamingTarget, chunk: String) -> Result<()> {
        let chat_id: i64 = target
            .channel
            .parse()
            .map_err(|_| LayersError::Channel("streaming requires numeric chat_id".into()))?;

        let mut state = self.streaming_state.lock().await;

        match &mut *state {
            Some((cid, mid, accumulated)) if *cid == chat_id => {
                // Append chunk and edit the existing message.
                accumulated.push_str(&chunk);
                let req = EditMessageTextRequest {
                    chat_id,
                    message_id: *mid,
                    text: accumulated.clone(),
                };

                let resp = self
                    .client
                    .post(self.api_url("editMessageText"))
                    .json(&req)
                    .send()
                    .await
                    .map_err(|e| {
                        LayersError::Channel(format!("telegram editMessageText failed: {e}"))
                    })?;

                let body: TgResponse<serde_json::Value> = resp.json().await.map_err(|e| {
                    LayersError::Channel(format!("telegram editMessageText parse error: {e}"))
                })?;

                if !body.ok {
                    warn!(
                        desc = ?body.description,
                        "editMessageText returned ok=false (may be rate-limited)"
                    );
                }
            }
            _ => {
                // First chunk — send a new message and store its message_id.
                let req = SendMessageRequest {
                    chat_id,
                    text: chunk.clone(),
                    reply_to_message_id: target
                        .message_id
                        .as_ref()
                        .and_then(|id| id.parse().ok()),
                };

                let resp = self
                    .client
                    .post(self.api_url("sendMessage"))
                    .json(&req)
                    .send()
                    .await
                    .map_err(|e| {
                        LayersError::Channel(format!("telegram sendMessage (stream) failed: {e}"))
                    })?;

                let body: TgResponse<SendMessageResult> = resp.json().await.map_err(|e| {
                    LayersError::Channel(format!("telegram sendMessage (stream) parse: {e}"))
                })?;

                if !body.ok {
                    return Err(LayersError::Channel(format!(
                        "telegram sendMessage (stream) error: {}",
                        body.description.unwrap_or_default()
                    )));
                }

                if let Some(result) = body.result {
                    *state = Some((chat_id, result.message_id, chunk));
                }
            }
        }

        Ok(())
    }

    async fn send_reaction(&self, channel: &str, message_id: &str, emoji: &str) -> Result<()> {
        let chat_id: i64 = channel
            .parse()
            .map_err(|_| LayersError::Channel("reaction requires numeric chat_id".into()))?;
        let msg_id: i64 = message_id
            .parse()
            .map_err(|_| LayersError::Channel("reaction requires numeric message_id".into()))?;

        let req = SetReactionRequest {
            chat_id,
            message_id: msg_id,
            reaction: vec![ReactionType {
                kind: "emoji".to_string(),
                emoji: emoji.to_string(),
            }],
        };

        let resp = self
            .client
            .post(self.api_url("setMessageReaction"))
            .json(&req)
            .send()
            .await
            .map_err(|e| {
                LayersError::Channel(format!("telegram setMessageReaction failed: {e}"))
            })?;

        let body: TgResponse<serde_json::Value> = resp.json().await.map_err(|e| {
            LayersError::Channel(format!("telegram setMessageReaction parse error: {e}"))
        })?;

        if !body.ok {
            return Err(LayersError::Channel(format!(
                "telegram setMessageReaction error: {}",
                body.description.unwrap_or_default()
            )));
        }

        Ok(())
    }

    async fn health(&self) -> ChannelHealth {
        if self.polling_active.load(Ordering::SeqCst) {
            ChannelHealth::Connected
        } else {
            ChannelHealth::Disconnected
        }
    }
}
