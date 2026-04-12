//! Cron execution engine.
//!
//! Evaluates cron schedules, computes next run times, enqueues due jobs
//! into session mailboxes, and tracks run history.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use layers_core::{
    CancellationToken, CronJob, InboundMessage, MisFirePolicy, PeerKind, SessionTarget,
};
use layers_runtime::queue::QueueManager;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// A record of a single cron job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRun {
    pub job_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: CronRunStatus,
    pub error: Option<String>,
}

/// Status of a cron run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CronRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

/// Tracks scheduled jobs and their next run times.
pub struct CronScheduler {
    jobs: RwLock<Vec<CronJob>>,
    next_runs: RwLock<HashMap<String, DateTime<Utc>>>,
    run_history: RwLock<Vec<CronRun>>,
    queue_manager: Arc<QueueManager>,
    cancel: CancellationToken,
    delete_after_run: RwLock<HashMap<String, bool>>,
}

impl CronScheduler {
    /// Create a new cron scheduler.
    #[must_use]
    pub fn new(queue_manager: Arc<QueueManager>, cancel: CancellationToken) -> Self {
        Self {
            jobs: RwLock::new(Vec::new()),
            next_runs: RwLock::new(HashMap::new()),
            run_history: RwLock::new(Vec::new()),
            queue_manager,
            cancel,
            delete_after_run: RwLock::new(HashMap::new()),
        }
    }

    /// Register a cron job.
    pub async fn add_job(&self, job: CronJob) {
        let next = compute_next_run(&job.schedule.cron, Utc::now());
        if let Some(next_time) = next {
            self.next_runs
                .write()
                .await
                .insert(job.id.clone(), next_time);
        }
        info!(job_id = %job.id, cron = %job.schedule.cron, "cron job registered");
        self.jobs.write().await.push(job);
    }

    /// Register a job that will be removed after its first successful execution.
    pub async fn add_job_delete_after_run(&self, job: CronJob) {
        self.delete_after_run
            .write()
            .await
            .insert(job.id.clone(), true);
        self.add_job(job).await;
    }

    /// Remove a job by ID.
    pub async fn remove_job(&self, job_id: &str) {
        self.jobs.write().await.retain(|j| j.id != job_id);
        self.next_runs.write().await.remove(job_id);
        self.delete_after_run.write().await.remove(job_id);
        info!(job_id = %job_id, "cron job removed");
    }

    /// Get the run history.
    pub async fn history(&self) -> Vec<CronRun> {
        self.run_history.read().await.clone()
    }

    /// Run the scheduler tick loop. Blocks until cancellation.
    pub async fn run(&self) {
        info!("cron scheduler started");
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));

        loop {
            tokio::select! {
                () = self.cancel.cancelled() => {
                    info!("cron scheduler stopping");
                    return;
                }
                _ = ticker.tick() => {
                    self.tick().await;
                }
            }
        }
    }

    /// Check all jobs and enqueue any that are due.
    async fn tick(&self) {
        let now = Utc::now();
        let jobs = self.jobs.read().await.clone();
        let mut to_delete = Vec::new();

        for job in &jobs {
            if !job.enabled {
                continue;
            }

            let is_due = {
                let runs = self.next_runs.read().await;
                runs.get(&job.id)
                    .is_some_and(|next_time| now >= *next_time)
            };

            if !is_due {
                continue;
            }

            debug!(job_id = %job.id, "cron job due — enqueuing");

            let session_id = match &job.session_target {
                Some(SessionTarget::Resume { session_id }) => session_id.clone(),
                _ => format!("cron:{}", job.id),
            };

            let msg = InboundMessage {
                channel: "cron".to_string(),
                channel_message_id: uuid::Uuid::new_v4().to_string(),
                peer_id: "cron-scheduler".to_string(),
                peer_display_name: "Cron".to_string(),
                peer_kind: PeerKind::System,
                text: job.payload.prompt.clone(),
                attachments: Vec::new(),
                thread_id: None,
                reply_to_message_id: None,
                channel_metadata: None,
                timestamp: now,
            };

            let queue = self.queue_manager.get_or_create(&session_id).await;

            // Apply misfire policy if session is active.
            if queue.is_active().await {
                let policy = job
                    .delivery
                    .as_ref()
                    .and_then(|d| d.misfire_policy.as_ref())
                    .unwrap_or(&MisFirePolicy::Queue);

                match policy {
                    MisFirePolicy::Skip => {
                        debug!(job_id = %job.id, "session active — skipping (misfire=skip)");
                        self.record_run(&job.id, CronRunStatus::Skipped, None)
                            .await;
                    }
                    MisFirePolicy::RunImmediately | MisFirePolicy::Queue => {
                        let _ = queue.enqueue(msg).await;
                        self.record_run(&job.id, CronRunStatus::Completed, None)
                            .await;
                    }
                }
            } else {
                let _ = queue.enqueue(msg).await;
                self.record_run(&job.id, CronRunStatus::Completed, None)
                    .await;
            }

            // Update next run.
            if let Some(next) = compute_next_run(&job.schedule.cron, now) {
                self.next_runs.write().await.insert(job.id.clone(), next);
            } else {
                warn!(job_id = %job.id, "could not compute next run — disabling");
                self.next_runs.write().await.remove(&job.id);
            }

            // Check delete_after_run.
            if self
                .delete_after_run
                .read()
                .await
                .get(&job.id)
                .copied()
                .unwrap_or(false)
            {
                to_delete.push(job.id.clone());
            }
        }

        for id in to_delete {
            self.remove_job(&id).await;
        }
    }

    async fn record_run(&self, job_id: &str, status: CronRunStatus, error: Option<String>) {
        let run = CronRun {
            job_id: job_id.to_string(),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            status,
            error,
        };
        self.run_history.write().await.push(run);
    }
}

/// Compute the next run time from a cron expression.
///
/// This is a simplified parser that handles basic `* * * * *` (min hour dom month dow)
/// patterns. For production use, integrate a full cron parser like `cron` or `croner`.
fn compute_next_run(expr: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() < 5 {
        warn!(expr = %expr, "invalid cron expression — expected 5 fields");
        return None;
    }

    // Parse the minute field for simple cases.
    if let Some(interval_mins) = parse_every_n_minutes(fields[0]) {
        let next = after + chrono::Duration::minutes(i64::from(interval_mins));
        return Some(next);
    }

    // Fallback: run one minute from now.
    Some(after + chrono::Duration::minutes(1))
}

/// Parse `*/N` from the minute field, returning `N`.
fn parse_every_n_minutes(field: &str) -> Option<u32> {
    let n_str = field.strip_prefix("*/")?;
    n_str.parse().ok()
}
