//! Message queue: actor/mailbox model per session with multiple queue modes.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, RwLock};

use layers_core::InboundMessage;

// ---------------------------------------------------------------------------
// Queue modes
// ---------------------------------------------------------------------------

/// How queued messages are handled when a run is already active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum QueueMode {
    /// Coalesce queued messages into a follow-up turn after current run finishes.
    #[default]
    Collect,
    /// Inject into current run at the next tool boundary.
    Steer,
    /// Enqueue for the next turn (no coalescing).
    Followup,
    /// Abort active run, run newest message immediately.
    Interrupt,
    /// Steer the current message + preserve backlog for followup.
    SteerBacklog,
}


// ---------------------------------------------------------------------------
// Queued entry
// ---------------------------------------------------------------------------

/// A message waiting in the session queue.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub message: InboundMessage,
    pub mode: QueueMode,
    pub queued_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Session queue (per-session mailbox)
// ---------------------------------------------------------------------------

/// Per-session mailbox that serializes runs and queues concurrent messages.
pub struct SessionQueue {
    /// Pending messages waiting for the next run.
    pending: Mutex<VecDeque<QueuedMessage>>,
    /// Whether a run is currently active for this session.
    active_run: Mutex<bool>,
    /// Default queue mode for this session.
    default_mode: QueueMode,
    /// Channel to notify when a queued message should steer the active run.
    steer_tx: Option<mpsc::Sender<QueuedMessage>>,
}

impl SessionQueue {
    pub fn new(default_mode: QueueMode) -> Self {
        Self {
            pending: Mutex::new(VecDeque::new()),
            active_run: Mutex::new(false),
            default_mode,
            steer_tx: None,
        }
    }

    /// Create a queue with a steer channel for injecting messages into active runs.
    pub fn with_steer_channel(default_mode: QueueMode) -> (Self, mpsc::Receiver<QueuedMessage>) {
        let (tx, rx) = mpsc::channel(64);
        let queue = Self {
            pending: Mutex::new(VecDeque::new()),
            active_run: Mutex::new(false),
            default_mode,
            steer_tx: Some(tx),
        };
        (queue, rx)
    }

    /// Enqueue a message. Returns `true` if it should start a new run (no active run).
    pub async fn enqueue(&self, message: InboundMessage) -> bool {
        self.enqueue_with_mode(message, self.default_mode).await
    }

    /// Enqueue with an explicit mode override.
    pub async fn enqueue_with_mode(&self, message: InboundMessage, mode: QueueMode) -> bool {
        let entry = QueuedMessage {
            message,
            mode,
            queued_at: chrono::Utc::now(),
        };

        let mut active = self.active_run.lock().await;

        if !*active {
            // No active run — push to pending and signal caller to start a run.
            self.pending.lock().await.push_back(entry);
            *active = true;
            return true;
        }

        // There's an active run — behavior depends on mode.
        match mode {
            QueueMode::Steer | QueueMode::SteerBacklog => {
                // Try to inject via steer channel.
                if let Some(ref tx) = self.steer_tx {
                    let _ = tx.send(entry).await;
                } else {
                    // Fall back to pending queue.
                    self.pending.lock().await.push_back(entry);
                }
            }
            QueueMode::Interrupt => {
                // Clear pending, push this as the only pending message.
                let mut pending = self.pending.lock().await;
                pending.clear();
                pending.push_back(entry);
                // Caller should cancel the active run — we signal via return false
                // but the interrupt handling is done by the run manager.
            }
            QueueMode::Collect | QueueMode::Followup => {
                self.pending.lock().await.push_back(entry);
            }
        }

        false
    }

    /// Drain all pending messages (called when starting a new run).
    pub async fn drain_pending(&self) -> Vec<QueuedMessage> {
        self.pending.lock().await.drain(..).collect()
    }

    /// Mark the current run as complete. Returns `true` if there are pending messages
    /// that need a new run.
    pub async fn run_complete(&self) -> bool {
        let mut active = self.active_run.lock().await;
        *active = false;
        !self.pending.lock().await.is_empty()
    }

    /// Check if there's an active run.
    pub async fn is_active(&self) -> bool {
        *self.active_run.lock().await
    }

    /// Get the number of pending messages.
    pub async fn pending_count(&self) -> usize {
        self.pending.lock().await.len()
    }
}

// ---------------------------------------------------------------------------
// Queue manager (multi-session)
// ---------------------------------------------------------------------------

/// Manages session queues across multiple sessions.
pub struct QueueManager {
    queues: RwLock<HashMap<String, Arc<SessionQueue>>>,
    default_mode: QueueMode,
}

impl QueueManager {
    pub fn new(default_mode: QueueMode) -> Self {
        Self {
            queues: RwLock::new(HashMap::new()),
            default_mode,
        }
    }

    /// Get or create the queue for a session.
    pub async fn get_or_create(&self, session_id: &str) -> Arc<SessionQueue> {
        // Fast path: read lock.
        {
            let queues = self.queues.read().await;
            if let Some(q) = queues.get(session_id) {
                return Arc::clone(q);
            }
        }

        // Slow path: write lock.
        let mut queues = self.queues.write().await;
        queues
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(SessionQueue::new(self.default_mode)))
            .clone()
    }

    /// Remove a session queue (e.g., on session archival).
    pub async fn remove(&self, session_id: &str) {
        self.queues.write().await.remove(session_id);
    }
}
