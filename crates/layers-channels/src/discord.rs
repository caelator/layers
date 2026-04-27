//! Discord adapter — REST API via reqwest (no serenity/twilight dependency).
//!
//! Uses the Discord REST API for outbound messaging (send, edit, react).
//! Inbound messages are received via the gateway webhook endpoint rather than
//! a WebSocket gateway connection, keeping dependencies minimal.

use async_trait::async_trait;
use layers_core::{
    CancellationToken, ChannelAdapter, ChannelHealth, LayersError, OutboundMessage, Result,
    StreamingTarget,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use tracing::{info, warn};

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

// ---------------------------------------------------------------------------
// Discord API types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CreateMessage {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_reference: Option<MessageReference>,
}

#[derive(Debug, Serialize)]
struct MessageReference {
    message_id: String,
}

#[derive(Debug, Serialize)]
struct EditMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct DiscordMessage {
    id: String,
    #[allow(dead_code)]
    channel_id: String,
}

#[derive(Debug, Deserialize)]
struct DiscordError {
    #[allow(dead_code)]
    code: Option<i64>,
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// Discord bot adapter using REST API.
pub struct DiscordAdapter {
    bot_token: String,
    client: Client,
    connected: AtomicBool,
    cancel: Mutex<Option<CancellationToken>>,
    /// Tracks streaming message state: (channel_id, message_id, accumulated_text).
    streaming_state: Mutex<Option<(String, String, String)>>,
}

impl DiscordAdapter {
    /// Create a new Discord adapter with the given bot token.
    #[must_use]
    pub fn new(bot_token: String) -> Self {
        Self {
            bot_token,
            client: Client::new(),
            connected: AtomicBool::new(false),
            cancel: Mutex::new(None),
            streaming_state: Mutex::new(None),
        }
    }

    /// Returns the configured bot token.
    #[must_use]
    pub fn bot_token(&self) -> &str {
        &self.bot_token
    }

    /// Build an Authorization header value.
    fn auth_header(&self) -> String {
        format!("Bot {}", self.bot_token)
    }

    /// Send a POST/PATCH/PUT to the Discord API and check for errors.
    async fn api_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<reqwest::Response> {
        let url = format!("{DISCORD_API_BASE}{path}");
        let mut req = self
            .client
            .request(method, &url)
            .header("Authorization", self.auth_header());

        if let Some(b) = body {
            req = req.json(b);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| LayersError::Channel(format!("discord API request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.json::<DiscordError>().await.unwrap_or(DiscordError {
                code: None,
                message: Some(format!("HTTP {status}")),
            });
            return Err(LayersError::Channel(format!(
                "discord API error ({}): {}",
                status,
                err_body.message.unwrap_or_default()
            )));
        }

        Ok(resp)
    }
}

#[async_trait]
impl ChannelAdapter for DiscordAdapter {
    fn name(&self) -> &str {
        "discord"
    }

    async fn start(&self, cancel: CancellationToken) -> Result<()> {
        info!("discord adapter starting — REST API ready, inbound via webhook");
        self.connected.store(true, Ordering::SeqCst);
        *self.cancel.lock().await = Some(cancel);
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("discord adapter stopping");
        if let Some(cancel) = self.cancel.lock().await.take() {
            cancel.cancel();
        }
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> Result<()> {
        let channel_id = &message.channel;
        let path = format!("/channels/{channel_id}/messages");

        let body = CreateMessage {
            content: message.text,
            message_reference: message
                .reply_to_message_id
                .map(|id| MessageReference { message_id: id }),
        };

        self.api_request(reqwest::Method::POST, &path, Some(&body))
            .await?;

        Ok(())
    }

    async fn send_streaming(&self, target: StreamingTarget, chunk: String) -> Result<()> {
        let channel_id = target.channel.clone();
        let mut state = self.streaming_state.lock().await;

        match &mut *state {
            Some((cid, mid, accumulated)) if *cid == channel_id => {
                // Append chunk and PATCH the existing message.
                accumulated.push_str(&chunk);
                let path = format!("/channels/{cid}/messages/{mid}");
                let body = EditMessage {
                    content: accumulated.clone(),
                };

                if let Err(e) = self
                    .api_request(reqwest::Method::PATCH, &path, Some(&body))
                    .await
                {
                    warn!(error = %e, "discord editMessage failed (rate limit?)");
                }
            }
            _ => {
                // First chunk — POST a new message and record its id.
                let path = format!("/channels/{channel_id}/messages");
                let body = CreateMessage {
                    content: chunk.clone(),
                    message_reference: target
                        .message_id
                        .map(|id| MessageReference { message_id: id }),
                };

                let resp = self
                    .api_request(reqwest::Method::POST, &path, Some(&body))
                    .await?;

                let msg: DiscordMessage = resp.json().await.map_err(|e| {
                    LayersError::Channel(format!("discord message parse error: {e}"))
                })?;

                *state = Some((channel_id, msg.id, chunk));
            }
        }

        Ok(())
    }

    async fn send_reaction(&self, channel: &str, message_id: &str, emoji: &str) -> Result<()> {
        // Discord requires URL-encoded emoji for the path. Use reqwest's
        // percent-encoding utilities since we already depend on reqwest.
        use reqwest::Url;
        // Build a dummy URL and extract the percent-encoded emoji segment.
        let dummy = format!("https://x/{emoji}");
        let encoded_emoji = Url::parse(&dummy)
            .map(|u| u.path().trim_start_matches('/').to_string())
            .unwrap_or_else(|_| emoji.to_string());
        let path =
            format!("/channels/{channel}/messages/{message_id}/reactions/{encoded_emoji}/@me");

        self.api_request(reqwest::Method::PUT, &path, None::<&()>)
            .await?;

        Ok(())
    }

    async fn health(&self) -> ChannelHealth {
        if self.connected.load(Ordering::SeqCst) {
            ChannelHealth::Connected
        } else {
            ChannelHealth::Disconnected
        }
    }
}
