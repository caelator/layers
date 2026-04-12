//! Channel-internal types and re-exports from layers-core.

pub use layers_core::{
    ChannelAdapter, ChannelHealth, ChannelMetadata, ChannelRuntimeHandle, InboundMessage,
    MediaAttachment, OutboundMessage, PeerKind, StreamingTarget,
};
use serde::{Deserialize, Serialize};

/// Per-channel model override configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelModelOverride {
    /// Channel name this override applies to.
    pub channel: String,
    /// Model reference string (e.g. "openai:gpt-4o").
    pub model: Option<String>,
    /// Per-account overrides keyed by peer_id.
    pub account_overrides: std::collections::HashMap<String, String>,
}

/// Deduplication key for inbound messages.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct DedupKey {
    pub channel: String,
    pub channel_message_id: String,
}

impl DedupKey {
    #[must_use]
    pub fn from_inbound(msg: &InboundMessage) -> Self {
        Self {
            channel: msg.channel.clone(),
            channel_message_id: msg.channel_message_id.clone(),
        }
    }
}
