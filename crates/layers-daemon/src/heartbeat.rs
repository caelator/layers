//! Periodic heartbeat scheduler.
//!
//! Sends heartbeat messages to session mailboxes on a configurable interval,
//! respecting active hours and skipping sessions that are already running.

use std::sync::Arc;
use std::time::Duration;

use chrono::{NaiveTime, Timelike, Utc};
use layers_core::{
    ActiveHours, CancellationToken, HeartbeatConfig, HumanDuration, InboundMessage, PeerKind,
};
use layers_runtime::queue::QueueManager;
use tracing::{debug, info, warn};

/// Heartbeat scheduler that periodically injects heartbeat messages.
pub struct HeartbeatScheduler {
    config: HeartbeatConfig,
    agent_id: String,
    queue_manager: Arc<QueueManager>,
    cancel: CancellationToken,
}

impl HeartbeatScheduler {
    /// Create a new heartbeat scheduler.
    #[must_use]
    pub fn new(
        config: HeartbeatConfig,
        agent_id: String,
        queue_manager: Arc<QueueManager>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            config,
            agent_id,
            queue_manager,
            cancel,
        }
    }

    /// Run the heartbeat loop. Blocks until cancellation.
    pub async fn run(&self) {
        let interval = parse_human_duration(&self.config.interval);
        info!(
            agent_id = %self.agent_id,
            interval_secs = interval.as_secs(),
            "heartbeat scheduler started"
        );

        let mut ticker = tokio::time::interval(interval);
        // Skip the first immediate tick.
        ticker.tick().await;

        loop {
            tokio::select! {
                () = self.cancel.cancelled() => {
                    info!(agent_id = %self.agent_id, "heartbeat scheduler stopping");
                    return;
                }
                _ = ticker.tick() => {
                    self.fire_heartbeat().await;
                }
            }
        }
    }

    async fn fire_heartbeat(&self) {
        // Check active hours.
        if let Some(ref hours) = self.config.active_hours {
            if !is_within_active_hours(hours) {
                debug!(agent_id = %self.agent_id, "outside active hours — skipping heartbeat");
                return;
            }
        }

        // Check if session is already running.
        let session_id = format!("heartbeat:{}", self.agent_id);
        let queue = self.queue_manager.get_or_create(&session_id).await;
        if queue.is_active().await {
            debug!(agent_id = %self.agent_id, "session already active — skipping heartbeat");
            return;
        }

        let msg = InboundMessage {
            channel: "heartbeat".to_string(),
            channel_message_id: uuid::Uuid::new_v4().to_string(),
            peer_id: "system".to_string(),
            peer_display_name: "Heartbeat".to_string(),
            peer_kind: PeerKind::System,
            text: format!("[heartbeat] agent={}", self.agent_id),
            attachments: Vec::new(),
            thread_id: None,
            reply_to_message_id: None,
            channel_metadata: None,
            timestamp: Utc::now(),
        };

        let should_start = queue.enqueue(msg).await;
        if should_start {
            info!(agent_id = %self.agent_id, "heartbeat enqueued — session should start");
        }
    }
}

/// Parse a `HumanDuration` string like "30s", "5m", "1h" into a `Duration`.
fn parse_human_duration(hd: &HumanDuration) -> Duration {
    let s = hd.0.trim();
    if let Some(num) = s.strip_suffix('s') {
        Duration::from_secs(num.parse().unwrap_or(60))
    } else if let Some(num) = s.strip_suffix('m') {
        Duration::from_secs(num.parse::<u64>().unwrap_or(5) * 60)
    } else if let Some(num) = s.strip_suffix('h') {
        Duration::from_secs(num.parse::<u64>().unwrap_or(1) * 3600)
    } else {
        warn!(raw = %s, "unrecognized duration format — defaulting to 60s");
        Duration::from_secs(60)
    }
}

/// Check if the current time falls within the configured active hours.
fn is_within_active_hours(hours: &ActiveHours) -> bool {
    let now = Utc::now();
    // Simplified — ignores timezone config for now, uses UTC.
    let current = NaiveTime::from_hms_opt(now.hour(), now.minute(), 0).unwrap_or_default();
    let start = parse_time_str(&hours.start);
    let end = parse_time_str(&hours.end);

    if start <= end {
        current >= start && current <= end
    } else {
        // Wraps midnight (e.g. 22:00 - 06:00).
        current >= start || current <= end
    }
}

fn parse_time_str(s: &str) -> NaiveTime {
    // Accepts "HH:MM" or "HH".
    let parts: Vec<&str> = s.split(':').collect();
    let hour: u32 = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
    let min: u32 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    NaiveTime::from_hms_opt(hour, min, 0).unwrap_or_default()
}
