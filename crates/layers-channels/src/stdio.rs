//! Stdio adapter — terminal/CLI channel for `layers chat` mode.
//!
//! Reads lines from stdin and writes responses to stdout.


use async_trait::async_trait;
use layers_core::{
    CancellationToken, ChannelAdapter, ChannelHealth, InboundMessage, LayersError, OutboundMessage,
    PeerKind, Result, StreamingTarget,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};
use tracing::info;

/// Terminal/CLI channel adapter.
///
/// Reads user input from stdin line-by-line and delivers it as inbound messages.
/// Writes assistant responses to stdout.
pub struct StdioAdapter {
    inbound_tx: mpsc::Sender<InboundMessage>,
    health: Mutex<ChannelHealth>,
    cancel: Mutex<Option<CancellationToken>>,
}

impl StdioAdapter {
    /// Create a new stdio adapter with the given inbound message sender.
    #[must_use]
    pub fn new(inbound_tx: mpsc::Sender<InboundMessage>) -> Self {
        Self {
            inbound_tx,
            health: Mutex::new(ChannelHealth::Disconnected),
            cancel: Mutex::new(None),
        }
    }

    /// Spawn the stdin reader loop. This runs until cancellation or EOF.
    fn spawn_reader(
        inbound_tx: mpsc::Sender<InboundMessage>,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let stdin = tokio::io::stdin();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            loop {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    line = lines.next_line() => {
                        match line {
                            Ok(Some(text)) => {
                                let text = text.trim().to_string();
                                if text.is_empty() {
                                    continue;
                                }
                                let msg = InboundMessage {
                                    channel: "stdio".to_string(),
                                    channel_message_id: uuid::Uuid::new_v4().to_string(),
                                    peer_id: "local-user".to_string(),
                                    peer_display_name: "User".to_string(),
                                    peer_kind: PeerKind::User,
                                    text,
                                    attachments: Vec::new(),
                                    thread_id: None,
                                    reply_to_message_id: None,
                                    channel_metadata: None,
                                    timestamp: chrono::Utc::now(),
                                };
                                if inbound_tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                            Ok(None) => break, // EOF
                            Err(_) => break,
                        }
                    }
                }
            }
        })
    }
}

#[async_trait]
impl ChannelAdapter for StdioAdapter {
    fn name(&self) -> &str {
        "stdio"
    }

    async fn start(&self, cancel: CancellationToken) -> Result<()> {
        info!("stdio adapter started — reading from stdin");
        *self.health.lock().await = ChannelHealth::Connected;
        *self.cancel.lock().await = Some(cancel.clone());
        let _handle = Self::spawn_reader(self.inbound_tx.clone(), cancel);
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("stdio adapter stopping");
        if let Some(cancel) = self.cancel.lock().await.take() {
            cancel.cancel();
        }
        *self.health.lock().await = ChannelHealth::Disconnected;
        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> Result<()> {
        let mut stdout = tokio::io::stdout();
        let output = format!("\n{}\n", message.text);
        stdout
            .write_all(output.as_bytes())
            .await
            .map_err(|e| LayersError::Channel(format!("stdout write error: {e}")))?;
        stdout
            .flush()
            .await
            .map_err(|e| LayersError::Channel(format!("stdout flush error: {e}")))?;
        Ok(())
    }

    async fn send_streaming(&self, _target: StreamingTarget, chunk: String) -> Result<()> {
        let mut stdout = tokio::io::stdout();
        stdout
            .write_all(chunk.as_bytes())
            .await
            .map_err(|e| LayersError::Channel(format!("stdout write error: {e}")))?;
        stdout
            .flush()
            .await
            .map_err(|e| LayersError::Channel(format!("stdout flush error: {e}")))?;
        Ok(())
    }

    async fn send_reaction(&self, _channel: &str, _message_id: &str, emoji: &str) -> Result<()> {
        let mut stdout = tokio::io::stdout();
        let output = format!("[{emoji}]\n");
        stdout
            .write_all(output.as_bytes())
            .await
            .map_err(|e| LayersError::Channel(format!("stdout write error: {e}")))?;
        stdout
            .flush()
            .await
            .map_err(|e| LayersError::Channel(format!("stdout flush error: {e}")))?;
        Ok(())
    }

    async fn health(&self) -> ChannelHealth {
        self.health.lock().await.clone()
    }
}
