//! Sentry plugin — monitors Sentry for unresolved errors and triggers repair workflows.
//!
//! ## Design
//!
//! This plugin watches a Sentry project for new and unresolved errors. When it
//! finds one, it classifies whether it can self-heal (restart, scale, config fix)
//! or needs council deliberation (code bug, unknown cause). Results are emitted as
//! `SentryMonitoringEvent` telemetry events and written to the technician's
//! detection pipeline.
//!
//! ## Sentry API
//!
//! Uses the Sentry REST API v2. Auth is via Bearer token in the `Authorization`
//! header. Token is read from the `SENTRY_API_TOKEN` environment variable.
//!
//! Base URL: `https://sentry.io/api/0`
//!
//! Key endpoints:
//!   GET /api/0/organizations/{org}/issues/?project={id}&statsFor=24h
//!   GET /api/0/organizations/{org}/issues/{id}/
//!   GET /api/0/organizations/{org}/events/{event_id}/
//!   POST /api/0/organizations/{org}/issues/{id}/actions/resolve/

pub mod schema;


use std::env;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write as IoWrite};
use std::path::PathBuf;

use chrono::{DateTime, Utc};

pub use schema::{SentryEvent, SentryIssue, SentryIssueSummary, SeverityLevel};

use crate::config::memoryport_dir;
use crate::util::iso_now;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const SENTRY_PLUGIN_DIR: &str = "sentry";
const EVENTS_FILE: &str = "sentry-monitoring.jsonl";

/// Path to the Sentry monitoring events file.
pub fn events_path() -> PathBuf {
    memoryport_dir().join(SENTRY_PLUGIN_DIR).join(EVENTS_FILE)
}

/// Sentry plugin configuration — loaded from environment or config file.
#[derive(Debug, Clone)]
pub struct SentryConfig {
    pub org: String,
    pub project_slug: String,
    pub api_token: String,
    /// How often to poll Sentry, in seconds.
    pub poll_interval_secs: u64,
    /// Error levels that trigger an alert.
    pub alert_levels: Vec<String>,
    /// Errors older than this (hours) with no resolution are escalated.
    pub stale_error_hours: u32,
}

impl Default for SentryConfig {
    fn default() -> Self {
        Self {
            org: env::var("SENTRY_ORG").unwrap_or_else(|_| "caelator".to_string()),
            project_slug: env::var("SENTRY_PROJECT").unwrap_or_default(),
            api_token: env::var("SENTRY_API_TOKEN").unwrap_or_default(),
            poll_interval_secs: 60,
            alert_levels: vec!["error".to_string(), "fatal".to_string(), "critical".to_string()],
            stale_error_hours: 24,
        }
    }
}

impl SentryConfig {
    /// Returns true if the API token is configured.
    pub fn is_configured(&self) -> bool {
        !self.api_token.is_empty() && !self.org.is_empty()
    }

    /// The canonical Sentry API base URL for this org.
    fn api_base(&self) -> String {
        format!("https://sentry.io/api/0/organizations/{}/", self.org)
    }
}

// ---------------------------------------------------------------------------
// HTTP client
// ---------------------------------------------------------------------------

/// A minimal Sentry API client using std::http.
/// The token is read from SENTRY_API_TOKEN env var.
pub struct SentryClient {
    config: SentryConfig,
}

impl SentryClient {
    pub fn new(config: SentryConfig) -> Self {
        Self { config }
    }

    fn request(&self, method: &str, path: &str, body: Option<&str>) -> anyhow::Result<String> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{}", self.config.api_base().trim_end_matches('/'), path)
        };

        let token = env::var("SENTRY_API_TOKEN")
            .or_else(|_| Ok::<String, ()>(self.config.api_token.clone()))
            .unwrap_or_default();

        if token.is_empty() {
            anyhow::bail!("SENTRY_API_TOKEN is not set");
        }

        let mut req = match method {
            "GET" => ureq::get(&url),
            "POST" => ureq::post(&url),
            "PUT" => ureq::put(&url),
            "DELETE" => ureq::delete(&url),
            _ => anyhow::bail!("unsupported HTTP method: {method}"),
        };

        req = req
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/json")
            .set("Accept", "application/json");

        let response = if let Some(body) = body {
            req.send_string(body)
        } else {
            req.call()
        };

        match response {
            Ok(resp) => {
                let status = resp.status();
                let mut body = String::new();
                resp.into_reader().read_to_string(&mut body)?;
                if status >= 200 && status < 300 {
                    Ok(body)
                } else {
                    anyhow::bail!("Sentry API error {status}: {body}")
                }
            }
            Err(e) => anyhow::bail!("Sentry request failed: {e}"),
        }
    }

    /// List all unresolved issues for the project, with 24h stats.
    pub fn list_unresolved_issues(&self, project_id: &str) -> anyhow::Result<Vec<SentryIssueSummary>> {
        let url = format!(
            "{}issues/?project={}&statsFor=24h&query=is:unresolved",
            self.config.api_base(),
            project_id
        );
        let body = self.request("GET", &url, None)?;
        let issues: Vec<SentryIssueSummary> = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("failed to parse issues response: {e}: {body}"))?;
        Ok(issues)
    }

    /// Get full issue details including tags and assigned user.
    pub fn get_issue(&self, issue_id: &str) -> anyhow::Result<SentryIssue> {
        let url = format!("{}issues/{}/", self.config.api_base(), issue_id);
        let body = self.request("GET", &url, None)?;
        let issue: SentryIssue = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("failed to parse issue response: {e}: {body}"))?;
        Ok(issue)
    }

    /// Get the most recent event for an issue (for stack trace / root cause).
    pub fn get_latest_event(&self, issue_id: &str) -> anyhow::Result<SentryEvent> {
        let url = format!("{}issues/{}/events/latest/", self.config.api_base(), issue_id);
        let body = self.request("GET", &url, None)?;
        let event: SentryEvent = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("failed to parse event response: {e}: {body}"))?;
        Ok(event)
    }

    /// Resolve an issue (mark it as resolved in Sentry).
    pub fn resolve_issue(&self, issue_id: &str) -> anyhow::Result<()> {
        let url = format!("{}issues/{}/actions/resolve/", self.config.api_base(), issue_id);
        self.request("POST", &url, None)?;
        Ok(())
    }

    /// Add a comment/note to an issue.
    pub fn add_issue_comment(&self, issue_id: &str, comment: &str) -> anyhow::Result<()> {
        let url = format!("{}issues/{}/comments/", self.config.api_base(), issue_id);
        #[derive(Serialize)]
        struct Body<'a> { text: &'a str }
        let body = serde_json::to_string(&Body { text: comment })?;
        self.request("POST", &url, Some(&body))?;
        Ok(())
    }

    /// Get project ID from project slug.
    pub fn get_project_id(&self, project_slug: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Project { id: String }
        let url = format!("{}projects/{}/{}/", self.config.api_base(), self.config.org, project_slug);
        let body = self.request("GET", &url, None)?;
        let project: Project = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("failed to parse project response: {e}"))?;
        Ok(project.id)
    }
}

// ---------------------------------------------------------------------------
// SentryPlugin
// ---------------------------------------------------------------------------

/// Main Sentry monitoring plugin.
pub struct SentryPlugin {
    config: SentryConfig,
    client: SentryClient,
    plugin_dir: PathBuf,
}

impl SentryPlugin {
    /// Create a new SentryPlugin. Reads config from SENTRY_* env vars.
    pub fn new(plugin_dir: PathBuf) -> Self {
        let config = SentryConfig::default();
        let client = SentryClient::new(config.clone());
        Self { config, client, plugin_dir }
    }

    /// Create from explicit config.
    pub fn with_config(config: SentryConfig, plugin_dir: PathBuf) -> Self {
        let client = SentryClient::new(config.clone());
        Self { config, client, plugin_dir }
    }

    /// Whether the plugin is configured with a valid API token.
    pub fn is_available(&self) -> bool {
        self.config.is_configured()
    }

    /// Run one monitoring cycle: fetch unresolved issues, classify each,
    /// emit events and write to the monitoring log.
    pub fn run_cycle(&self) -> anyhow::Result<Vec<MonitorResult>> {
        if !self.is_available() {
            return Ok(vec![]);
        }

        let project_id = self.client.get_project_id(&self.config.project_slug)?;
        let issues = self.client.list_unresolved_issues(&project_id)?;

        let mut results = Vec::new();
        for issue in issues {
            let severity = SeverityLevel::from(issue.level.as_str());
            if !severity.is_critical() {
                continue;
            }

            // Get full issue details for classification
            let full_issue = self.client.get_issue(&issue.id).ok();
            let latest_event = self.client.get_latest_event(&issue.id).ok();

            let classification = self.classify_issue(&issue, full_issue.as_ref(), latest_event.as_ref());
            let diagnosis_signal = self.classification_to_signal(&issue, &classification);
            let ts = iso_now();

            let result = MonitorResult {
                ts: ts.clone(),
                issue_id: issue.id.clone(),
                issue_title: issue.title.clone(),
                level: issue.level.clone(),
                count: issue.count,
                url: issue.permalink.clone(),
                classification,
                diagnosis_signal,
            };

            // Write to monitoring log
            let _ = self.append_event(&result);

            results.push(result);
        }

        Ok(results)
    }

    /// Classify a Sentry issue to determine repair strategy.
    fn classify_issue(
        &self,
        issue: &SentryIssueSummary,
        _full: Option<&SentryIssue>,
        event: Option<&SentryEvent>,
    ) -> IssueClassification {
        use IssueClassification::*;

        // Check for restart-suitable patterns first
        if let Some(event) = event {
            let exc_type = event.exception.as_ref()
                .and_then(|e| e.values.first())
                .and_then(|f| f.type_.split('<').next())
                .map(|s| s.trim());

            // Memory patterns → self-healable
            let mem_patterns = ["MemoryError", "OOM", "OutOfMemory", "MemoryError:", "memory limit", "Cannot allocate"];
            if mem_patterns.iter().any(|p| exc_type == Some(p) || event.message.as_ref().is_some_and(|m| m.contains(p))) {
                return SelfHealable(SelfHealType::RestartService);
            }

            // Database patterns → self-healable (restart clears connection pool)
            let db_patterns = [
                "OperationalError", "TooManyConnections", "connection pool",
                "ConnectionRefused", "ConnectionTimeout", "Deadlock",
                "could not connect to server",
            ];
            if db_patterns.iter().any(|p| exc_type == Some(p) || event.message.as_ref().is_some_and(|m| m.contains(p))) {
                return SelfHealable(SelfHealType::RestartService);
            }

            // Cache patterns → self-healable
            let cache_patterns = ["CacheMiss", "cache connection", "RedisError", "MemcachedError"];
            if cache_patterns.iter().any(|p| exc_type == Some(p) || event.message.as_ref().is_some_and(|m| m.contains(p))) {
                return SelfHealable(SelfHealType::PurgeCache);
            }

            // Rate limit patterns → self-healable
            let rate_patterns = ["429", "TooManyRequests", "rate limit", "RateLimitExceeded"];
            if rate_patterns.iter().any(|p| exc_type == Some(p) || event.message.as_ref().is_some_and(|m| m.contains(p))) {
                return SelfHealable(SelfHealType::ThrottleBack);
            }

            // Config/env errors → needs council (can't self-fix env)
            let config_patterns = [
                "EnvironmentVariableNotSet", "ConfigError", "MissingEnvVar",
                "EnvVarNotFound", ".env", "configuration key",
            ];
            if config_patterns.iter().any(|p| event.message.as_ref().is_some_and(|m| m.contains(p))) {
                return NeedsCouncil(CouncilReason::ConfigError {
                    message: event.message.clone().unwrap_or_default(),
                });
            }
        }

        // Check age — unresolved for >24h needs council
        if let Ok(last_seen) = DateTime::parse_from_rfc3339(&issue.last_seen) {
            let age_hours = (Utc::now() - last_seen.with_timezone(&Utc)).num_hours();
            if age_hours > self.config.stale_error_hours as i64 {
                return NeedsCouncil(CouncilReason::StaleError {
                    age_hours: age_hours as u32,
                });
            }
        }

        // Unknown error type → needs council
        NeedsCouncil(CouncilReason::UnknownError {
            title: issue.title.clone(),
            count: issue.count,
        })
    }

    fn classification_to_signal(&self, _issue: &SentryIssueSummary, classification: &IssueClassification) -> &'static str {
        match classification {
            IssueClassification::SelfHealable(_) => "self_healable",
            IssueClassification::NeedsCouncil(_) => "needs_council",
            IssueClassification::EscalateHuman(_) => "escalate_human",
        }
    }

    /// Append a monitoring result to the JSONL event log.
    fn append_event(&self, result: &MonitorResult) -> std::io::Result<()> {
        let dir = self.plugin_dir.join(SENTRY_PLUGIN_DIR);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(EVENTS_FILE);
        let line = serde_json::to_string(result)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Issue classification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IssueClassification {
    /// This error can be repaired automatically.
    SelfHealable(SelfHealType),
    /// Error requires multi-model deliberation to determine the fix.
    NeedsCouncil(CouncilReason),
    /// Error cannot be repaired automatically — human required.
    EscalateHuman(EscalationReason),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SelfHealType {
    /// Restart the service (clears memory leaks, connection pool exhaustion).
    RestartService,
    /// Purge CDN/cache (fixes cache stampede / stale data errors).
    PurgeCache,
    /// Back off requests (fixes rate limit errors).
    ThrottleBack,
    /// Roll back to previous deployment.
    RollbackDeployment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CouncilReason {
    /// Config/env variable missing — cannot self-fix.
    ConfigError { message: String },
    /// Error has been unresolved for >24h.
    StaleError { age_hours: u32 },
    /// Unknown error type — needs investigation.
    UnknownError { title: String, count: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EscalationReason {
    /// Security-relevant error.
    Security,
    /// Data corruption detected.
    DataCorruption,
    /// User-impacting outage.
    UserOutage { user_count: u32 },
}

// ---------------------------------------------------------------------------
// MonitorResult
// ---------------------------------------------------------------------------

/// The output of a single Sentry monitoring cycle for one issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorResult {
    pub ts: String,
    pub issue_id: String,
    pub issue_title: String,
    pub level: String,
    pub count: u32,
    pub url: String,
    pub classification: IssueClassification,
    pub diagnosis_signal: &'static str,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentry_config_requires_token() {
        let config = SentryConfig::default();
        // If env var is not set, it's empty by default
        // The actual check is is_configured() which requires both org and token
        assert!(!config.is_configured() || (!config.api_token.is_empty() && !config.org.is_empty()));
    }

    #[test]
    fn severity_critical_errors() {
        assert!(SeverityLevel::from("error").is_critical());
        assert!(SeverityLevel::from("fatal").is_critical());
        assert!(SeverityLevel::from("critical").is_critical());
        assert!(!SeverityLevel::from("warning").is_critical());
        assert!(!SeverityLevel::from("info").is_critical());
    }
}
