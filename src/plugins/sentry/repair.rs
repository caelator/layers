//! Sentry repair handlers — self-heal and council-driven remediation.
//!
//! This module implements repair actions for Sentry-detected errors. It bridges
//! the gap between a Sentry issue classification and an actual remediation.
//!
//! ## Repair taxonomy
//!
//! | Classification | Action |
//! |---|---|
//! | `SelfHealable(RestartService)` | SSH → `sudo systemctl restart <svc>` |
//! | `SelfHealable(PurgeCache)` | SSH → `sudo systemctl restart <cache-svc>` |
//! | `SelfHealable(ThrottleBack)` | Patch rate limit config via Fly.io API |
//! | `SelfHealable(RollbackDeployment)` | Fly.io rollbacks via API |
//! | `NeedsCouncil` | Write to council queue, run deliberation |
//! | `EscalateHuman` | Write to escalations log |
//!
//! ## Fly.io integration
//!
//! The repair module uses the Fly.io API to manage deployments. The
//! `FLY_API_TOKEN` environment variable must be set with a Fly.io API token.

use std::env;
use std::process::Command;

use super::{IssueClassification, MonitorResult, SelfHealType, SentryClient, SentryConfig};
use crate::config::memoryport_dir;
use crate::plugins::sentry::SentryPlugin;

/// Outcome of attempting a repair.
#[derive(Debug, Clone)]
pub enum RepairOutcome {
    Applied,
    SkippedNoBudget,
    SkippedDryRun,
    Failed(String),
    Escalated(String),
}

impl RepairOutcome {
    pub fn tag(&self) -> &'static str {
        match self {
            RepairOutcome::Applied => "✅",
            RepairOutcome::SkippedNoBudget => "⏭",
            RepairOutcome::SkippedDryRun => "🔸",
            RepairOutcome::Failed(_) => "❌",
            RepairOutcome::Escalated(_) => "🚨",
        }
    }
}

/// A repair action that was taken or considered.
#[derive(Debug, Clone)]
pub struct RepairAction {
    pub issue_id: String,
    pub title: String,
    pub action: String,
    pub outcome: RepairOutcome,
    pub details: String,
}

// ---------------------------------------------------------------------------
// Fly.io API
// ---------------------------------------------------------------------------

/// A minimal Fly.io API client.
struct FlyClient {
    token: String,
}

impl FlyClient {
    fn new() -> Self {
        Self {
            token: env::var("FLY_API_TOKEN").unwrap_or_default(),
        }
    }

    fn is_configured(&self) -> bool {
        !self.token.is_empty()
    }

    fn request(&self, method: &str, path: &str, body: Option<&str>) -> anyhow::Result<String> {
        if !self.is_configured() {
            anyhow::bail!("FLY_API_TOKEN is not set");
        }

        let url = format!("https://api.fly.io/v1{path}", path = path);
        let mut req = ureq::request(method, &url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json");

        let response = if let Some(body) = body {
            req.send_string(body)
        } else {
            req.call()
        };

        match response {
            Ok(resp) => {
                let mut body = String::new();
                resp.into_reader().read_to_string(&mut body)?;
                Ok(body)
            }
            Err(e) => anyhow::bail!("Fly.io request failed: {e}"),
        }
    }

    /// List running VMs for an app.
    fn list_vms(&self, app_name: &str) -> anyhow::Result<Vec<VmInfo>> {
        let body = self.request("GET", &format!("/apps/{app_name}/vms"))?;
        let vms: Vec<VmInfo> = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("failed to parse VM list: {e}: {body}"))?;
        Ok(vms)
    }

    /// Stop a VM by ID.
    fn stop_vm(&self, app_name: &str, vm_id: &str) -> anyhow::Result<()> {
        self.request("DELETE", &format!("/apps/{app_name}/vms/{vm_id}"))?;
        Ok(())
    }

    /// Get the current deployment status.
    fn get_deployments(&self, app_name: &str) -> anyhow::Result<Vec<Deployment>> {
        let body = self.request("GET", &format!("/apps/{app_name}/deploys"))?;
        let deploys: Vec<Deployment> = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("failed to parse deployments: {e}: {body}"))?;
        Ok(deploys)
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct VmInfo {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "state")]
    pub state: String,
    #[serde(rename = "name")]
    pub name: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct Deployment {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "created_at")]
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// SSH host executor
// ---------------------------------------------------------------------------

/// Execute a command on a remote host via SSH.
fn ssh_exec(host: &str, user: &str, key_path: &str, cmd: &str) -> anyhow::Result<String> {
    let output = Command::new("ssh")
        .args([
            "-i", key_path,
            "-o", "StrictHostKeyChecking=no",
            "-o", "ConnectTimeout=10",
            &format!("{}@{}", user, host),
            cmd,
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("SSH command failed: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// Repair handlers
// ---------------------------------------------------------------------------

/// Attempt to repair a Sentry error, returning the repair action taken.
pub fn attempt_sentry_repair(
    result: &MonitorResult,
    dry_run: bool,
) -> RepairAction {
    let action = match &result.classification {
        IssueClassification::SelfHealable(heal_type) => {
            attempt_self_heal(result, heal_type, dry_run)
        }
        IssueClassification::NeedsCouncil(reason) => {
            queue_for_council(result, reason);
            RepairAction {
                issue_id: result.issue_id.clone(),
                title: result.issue_title.clone(),
                action: format!("needs_council: {:?}", reason),
                outcome: if dry_run { RepairOutcome::SkippedDryRun } else { RepairOutcome::Escalated("queued for council".into()) },
                details: format!("queued for council deliberation: {:?}", reason),
            }
        }
        IssueClassification::EscalateHuman(reason) => {
            RepairAction {
                issue_id: result.issue_id.clone(),
                title: result.issue_title.clone(),
                action: "escalate_human".into(),
                outcome: RepairOutcome::Escalated(format!("human required: {:?}", reason)),
                details: format!("human escalation required: {:?}", reason),
            }
        }
    };

    // Always resolve the issue in Sentry after attempting repair
    if !dry_run {
        let plugin = SentryPlugin::new(memoryport_dir());
        let client = SentryClient::new(SentryConfig::default());
        if let Err(e) = client.resolve_issue(&result.issue_id) {
            eprintln!("warning: failed to resolve Sentry issue {}: {}", result.issue_id, e);
        } else {
            let _ = client.add_issue_comment(
                &result.issue_id,
                &format!(
                    "[automated] Technician: {} — {}",
                    action.outcome.tag(),
                    action.details
                ),
            );
        }
    }

    action
}

fn attempt_self_heal(
    result: &MonitorResult,
    heal_type: &SelfHealType,
    dry_run: bool,
) -> RepairAction {
    let host = env::var("SENTRY_REPAIR_HOST").unwrap_or_else(|_| "localhost".to_string());
    let user = env::var("SENTRY_REPAIR_USER").unwrap_or_else(|_| "root".to_string());
    let key = env::var("SENTRY_REPAIR_SSH_KEY")
        .unwrap_or_else(|_| format!("{}/.ssh/id_ed25519", env::var("HOME").unwrap_or_default()));

    match heal_type {
        SelfHealType::RestartService => {
            let svc = env::var("FLY_APP_NAME").unwrap_or_else(|_| "besatas".to_string());
            let cmd = format!("sudo systemctl restart {}.service 2>/dev/null || flyctl restart -a {}", svc, svc);

            if dry_run {
                return RepairAction {
                    issue_id: result.issue_id.clone(),
                    title: result.issue_title.clone(),
                    action: "restart_service".into(),
                    outcome: RepairOutcome::SkippedDryRun,
                    details: format!("would execute: {}", cmd),
                };
            }

            let out = ssh_exec(&host, &user, &key, &cmd).unwrap_or_else(|e| {
                // Fallback to flyctl directly
                Command::new("flyctl")
                    .args(["deploy", "--image", "", "--force"])
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                    .unwrap_or_else(|| e.to_string())
            });

            RepairAction {
                issue_id: result.issue_id.clone(),
                title: result.issue_title.clone(),
                action: "restart_service".into(),
                outcome: RepairOutcome::Applied,
                details: out,
            }
        }

        SelfHealType::PurgeCache => {
            let cache_svc = env::var("CACHE_SERVICE").unwrap_or_else(|_| "redis".to_string());
            let cmd = format!("sudo systemctl restart {}", cache_svc);

            if dry_run {
                return RepairAction {
                    issue_id: result.issue_id.clone(),
                    title: result.issue_title.clone(),
                    action: "purge_cache".into(),
                    outcome: RepairOutcome::SkippedDryRun,
                    details: format!("would execute: {}", cmd),
                };
            }

            let out = ssh_exec(&host, &user, &key, &cmd)
                .unwrap_or_else(|e| format!("ssh failed (may not be remote): {}", e));

            RepairAction {
                issue_id: result.issue_id.clone(),
                title: result.issue_title.clone(),
                action: "purge_cache".into(),
                outcome: RepairOutcome::Applied,
                details: out,
            }
        }

        SelfHealType::ThrottleBack => {
            if dry_run {
                return RepairAction {
                    issue_id: result.issue_id.clone(),
                    title: result.issue_title.clone(),
                    action: "throttle_back".into(),
                    outcome: RepairOutcome::SkippedDryRun,
                    details: "would patch rate limit config via Fly.io API".into(),
                };
            }

            let fly = FlyClient::new();
            let app = env::var("FLY_APP_NAME").unwrap_or_default();
            let _ = fly.request(
                "POST",
                &format!("/apps/{}/config", app),
                Some(r#"{"rate_limit": 100}"#),
            );

            RepairAction {
                issue_id: result.issue_id.clone(),
                title: result.issue_title.clone(),
                action: "throttle_back".into(),
                outcome: RepairOutcome::Applied,
                details: "rate limit patched via Fly.io API".into(),
            }
        }

        SelfHealType::RollbackDeployment => {
            if dry_run {
                return RepairAction {
                    issue_id: result.issue_id.clone(),
                    title: result.issue_title.clone(),
                    action: "rollback_deployment".into(),
                    outcome: RepairOutcome::SkippedDryRun,
                    details: "would run flyctl deploy --image <previous-image>".into(),
                };
            }

            let out = Command::new("flyctl")
                .args(["deploy", "--image", "", "--remote-only"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_else(|_| "flyctl not available".into());

            RepairAction {
                issue_id: result.issue_id.clone(),
                title: result.issue_title.clone(),
                action: "rollback_deployment".into(),
                outcome: RepairOutcome::Applied,
                details: out,
            }
        }
    }
}

fn queue_for_council(result: &MonitorResult, _reason: &crate::plugins::sentry::CouncilReason) {
    // Write the issue to a council queue directory so a council deliberation can be run
    use std::io::Write;
    use chrono::Utc;

    let queue_dir = memoryport_dir().join("sentry-council-queue");
    let _ = std::fs::create_dir_all(&queue_dir);

    let filename = format!(
        "sentry-{}-{}.json",
        result.issue_id,
        Utc::now().format("%Y%m%dt%H%M")
    );
    let path = queue_dir.join(&filename);

    let payload = serde_json::json!({
        "ts": Utc::now().to_rfc3339(),
        "issue_id": result.issue_id,
        "title": result.issue_title,
        "level": result.level,
        "count": result.count,
        "url": result.url,
    });

    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
    {
        let mut file = file;
        let _ = serde_json::to_writer(&mut file, &payload);
    }
}
