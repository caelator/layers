//! Daemon lifecycle — startup, shutdown, signal handling, PID file management.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use layers_channels::manager::ChannelManager;
use layers_core::{CancellationToken, DaemonConfig, LayersError, Result};
use layers_runtime::queue::{QueueManager, QueueMode};
use tokio::signal;
use tracing::{error, info, warn};

use crate::cron_scheduler::CronScheduler;
use crate::gateway::{Gateway, GatewayConfig};
use crate::hooks::HookManager;

/// Log rotation configuration.
#[derive(Debug, Clone)]
pub struct LogRotationConfig {
    pub max_size_bytes: u64,
    pub max_files: usize,
    pub directory: PathBuf,
}

impl Default for LogRotationConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: 10 * 1024 * 1024, // 10 MB
            max_files: 5,
            directory: PathBuf::from("/tmp/layers/logs"),
        }
    }
}

/// Orchestrates all daemon subsystems.
pub struct DaemonRunner {
    config: DaemonConfig,
    cancel: CancellationToken,
    pid_file: Option<PathBuf>,
    channel_manager: Arc<ChannelManager>,
    queue_manager: Arc<QueueManager>,
    hook_manager: Arc<HookManager>,
    cron_scheduler: Arc<CronScheduler>,
    log_rotation: Option<LogRotationConfig>,
}

impl DaemonRunner {
    /// Create a new daemon runner.
    ///
    /// Returns the runner and a receiver for inbound messages from channels.
    #[must_use]
    pub fn new(
        config: DaemonConfig,
    ) -> (Self, tokio::sync::mpsc::Receiver<layers_core::InboundMessage>) {
        let cancel = CancellationToken::new();
        let (channel_manager, inbound_rx) = ChannelManager::new(256, 500);
        let channel_manager = Arc::new(channel_manager);
        let queue_manager = Arc::new(QueueManager::new(QueueMode::Collect));
        let hook_manager = Arc::new(HookManager::new());
        let cron_scheduler = Arc::new(CronScheduler::new(
            Arc::clone(&queue_manager),
            cancel.clone(),
        ));

        let runner = Self {
            config,
            cancel,
            pid_file: None,
            channel_manager,
            queue_manager,
            hook_manager,
            cron_scheduler,
            log_rotation: None,
        };

        (runner, inbound_rx)
    }

    /// Set the PID file path.
    #[must_use]
    pub fn with_pid_file(mut self, path: PathBuf) -> Self {
        self.pid_file = Some(path);
        self
    }

    /// Set log rotation config.
    #[must_use]
    pub fn with_log_rotation(mut self, config: LogRotationConfig) -> Self {
        self.log_rotation = Some(config);
        self
    }

    /// Access the channel manager for adapter registration.
    #[must_use]
    pub fn channel_manager(&self) -> &Arc<ChannelManager> {
        &self.channel_manager
    }

    /// Access the queue manager.
    #[must_use]
    pub fn queue_manager(&self) -> &Arc<QueueManager> {
        &self.queue_manager
    }

    /// Access the hook manager.
    #[must_use]
    pub fn hook_manager(&self) -> &Arc<HookManager> {
        &self.hook_manager
    }

    /// Access the cron scheduler.
    #[must_use]
    pub fn cron_scheduler(&self) -> &Arc<CronScheduler> {
        &self.cron_scheduler
    }

    /// Access the cancellation token.
    #[must_use]
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Start the daemon and all subsystems. Blocks until shutdown.
    ///
    /// # Errors
    /// Returns an error if any subsystem fails to start.
    pub async fn run(&self) -> Result<()> {
        info!("daemon starting");

        // Write PID file.
        if let Some(ref pid_path) = self.pid_file {
            write_pid_file(pid_path).await?;
        }

        // Start channel adapters.
        self.channel_manager
            .start_all(self.cancel.clone())
            .await?;

        // Build and spawn the gateway.
        let gateway_config = GatewayConfig::from(&self.config);
        let gateway = Gateway::new(gateway_config, Arc::clone(&self.channel_manager));

        let gateway_cancel = self.cancel.clone();
        let gateway_handle = tokio::spawn(async move {
            tokio::select! {
                result = gateway.serve() => {
                    if let Err(e) = result {
                        error!(error = %e, "gateway exited with error");
                    }
                }
                () = gateway_cancel.cancelled() => {
                    info!("gateway shutting down");
                }
            }
        });

        // Spawn cron scheduler.
        let cron = Arc::clone(&self.cron_scheduler);
        let cron_handle = tokio::spawn(async move {
            cron.run().await;
        });

        // Wait for shutdown signal.
        info!(
            addr = %format!("{}:{}", self.config.bind_address, self.config.port),
            "daemon running"
        );

        wait_for_shutdown(self.cancel.clone()).await;

        // Graceful shutdown sequence.
        info!("initiating graceful shutdown");
        self.cancel.cancel();

        // Stop channels.
        if let Err(e) = self.channel_manager.stop_all().await {
            warn!(error = %e, "error stopping channel adapters");
        }

        // Wait for spawned tasks.
        let _ = gateway_handle.await;
        let _ = cron_handle.await;

        // Remove PID file.
        if let Some(ref pid_path) = self.pid_file {
            remove_pid_file(pid_path).await;
        }

        info!("daemon shutdown complete");
        Ok(())
    }
}

/// Wait for SIGTERM or SIGINT.
async fn wait_for_shutdown(cancel: CancellationToken) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received SIGINT"),
        () = terminate => info!("received SIGTERM"),
        () = cancel.cancelled() => info!("cancellation requested"),
    }
}

/// Write the current process PID to a file.
async fn write_pid_file(path: &Path) -> Result<()> {
    let pid = std::process::id().to_string();
    tokio::fs::write(path, pid.as_bytes())
        .await
        .map_err(|e| LayersError::Channel(format!("failed to write PID file: {e}")))?;
    info!(path = %path.display(), "PID file written");
    Ok(())
}

/// Remove the PID file.
async fn remove_pid_file(path: &Path) {
    if let Err(e) = tokio::fs::remove_file(path).await {
        warn!(path = %path.display(), error = %e, "failed to remove PID file");
    }
}
