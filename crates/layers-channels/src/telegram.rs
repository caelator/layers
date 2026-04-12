//! Telegram adapter — stub for teloxide integration.
//!
//! All methods are defined but return `unimplemented!()` pending
//! teloxide dependency and bot token configuration.

use async_trait::async_trait;
use layers_core::{
    CancellationToken, ChannelAdapter, ChannelHealth, LayersError, OutboundMessage, Result,
    StreamingTarget,
};
use tracing::warn;

/// Telegram bot adapter (stub).
pub struct TelegramAdapter {
    bot_token: String,
}

impl TelegramAdapter {
    /// Create a new Telegram adapter with the given bot token.
    #[must_use]
    pub fn new(bot_token: String) -> Self {
        Self { bot_token }
    }

    /// Returns the configured bot token.
    #[must_use]
    pub fn bot_token(&self) -> &str {
        &self.bot_token
    }
}

#[async_trait]
impl ChannelAdapter for TelegramAdapter {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&self, _cancel: CancellationToken) -> Result<()> {
        warn!("telegram adapter is a stub — not starting");
        Err(LayersError::Channel(
            "telegram adapter not yet implemented".into(),
        ))
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn send(&self, _message: OutboundMessage) -> Result<()> {
        unimplemented!("telegram send not yet implemented")
    }

    async fn send_streaming(&self, _target: StreamingTarget, _chunk: String) -> Result<()> {
        unimplemented!("telegram send_streaming not yet implemented")
    }

    async fn send_reaction(&self, _channel: &str, _message_id: &str, _emoji: &str) -> Result<()> {
        unimplemented!("telegram send_reaction not yet implemented")
    }

    async fn health(&self) -> ChannelHealth {
        ChannelHealth::Disconnected
    }
}
