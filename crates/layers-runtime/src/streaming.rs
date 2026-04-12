//! Block streaming: event delivery to channels and chunk assembly.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use crate::agent_loop::RunStatus;

// ---------------------------------------------------------------------------
// Stream events
// ---------------------------------------------------------------------------

/// Events emitted during an agent run for streaming consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    /// Run started.
    LifecycleStart { session_id: String },
    /// Incremental text from the assistant.
    TextDelta(String),
    /// Tool execution started.
    ToolStart { id: String, name: String },
    /// Tool execution completed.
    ToolEnd { id: String, name: String },
    /// Run finished.
    LifecycleEnd {
        session_id: String,
        status: RunStatus,
    },
}

// ---------------------------------------------------------------------------
// Streaming mode
// ---------------------------------------------------------------------------

/// How streaming is delivered to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamingMode {
    /// No streaming — wait for full response.
    Off,
    /// Emit partial text deltas only.
    Partial,
    /// Emit full block-level events (text, tool start/end, lifecycle).
    Block,
    /// Progress-only: lifecycle + tool events, no text deltas.
    Progress,
}

impl Default for StreamingMode {
    fn default() -> Self {
        Self::Off
    }
}

// ---------------------------------------------------------------------------
// Stream sink trait
// ---------------------------------------------------------------------------

/// Consumer of stream events. Implementations deliver events to channels, WebSocket, etc.
#[async_trait]
pub trait StreamSink: Send + Sync {
    async fn emit(&self, event: StreamEvent);
}

// ---------------------------------------------------------------------------
// Broadcast sink (multi-consumer)
// ---------------------------------------------------------------------------

/// Delivers stream events to multiple consumers via a broadcast channel.
pub struct BroadcastSink {
    tx: broadcast::Sender<StreamEvent>,
}

impl BroadcastSink {
    pub fn new(capacity: usize) -> (Self, broadcast::Receiver<StreamEvent>) {
        let (tx, rx) = broadcast::channel(capacity);
        (Self { tx }, rx)
    }

    /// Subscribe a new receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.tx.subscribe()
    }
}

#[async_trait]
impl StreamSink for BroadcastSink {
    async fn emit(&self, event: StreamEvent) {
        let _ = self.tx.send(event);
    }
}

// ---------------------------------------------------------------------------
// Mpsc sink (single consumer)
// ---------------------------------------------------------------------------

/// Delivers stream events to a single consumer via an mpsc channel.
pub struct MpscSink {
    tx: mpsc::Sender<StreamEvent>,
}

impl MpscSink {
    pub fn new(buffer: usize) -> (Self, mpsc::Receiver<StreamEvent>) {
        let (tx, rx) = mpsc::channel(buffer);
        (Self { tx }, rx)
    }
}

#[async_trait]
impl StreamSink for MpscSink {
    async fn emit(&self, event: StreamEvent) {
        let _ = self.tx.send(event).await;
    }
}

// ---------------------------------------------------------------------------
// Chunk assembler
// ---------------------------------------------------------------------------

/// Assembles streamed text deltas into complete blocks.
pub struct ChunkAssembler {
    buffer: String,
}

impl ChunkAssembler {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Append a text delta.
    pub fn push(&mut self, delta: &str) {
        self.buffer.push_str(delta);
    }

    /// Take the assembled text, resetting the buffer.
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.buffer)
    }

    /// Current buffer length.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

impl Default for ChunkAssembler {
    fn default() -> Self {
        Self::new()
    }
}
