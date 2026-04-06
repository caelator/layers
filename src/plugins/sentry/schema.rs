//! Sentry API schema types.

use serde::{Deserialize, Serialize};

/// An unresolved error issue from Sentry's API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryIssue {
    pub id: String,
    pub title: String,
    pub culprit: String,
    #[serde(rename = "type")]
    pub issue_type: String,
    pub level: String,
    pub platform: String,
    pub count: u32,
    pub user_count: u32,
    #[serde(rename = "firstSeen")]
    pub first_seen: String,
    #[serde(rename = "lastSeen")]
    pub last_seen: String,
    #[serde(rename = "isUnhandled")]
    pub is_unhandled: bool,
    #[serde(rename = "isPublic")]
    pub is_public: bool,
    #[serde(rename = "shortId")]
    pub short_id: String,
    pub permalink: String,
    #[serde(rename = "assignedTo")]
    pub assigned_to: Option<SentryUser>,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "project")]
    pub project: ProjectRef,
    #[serde(default)]
    pub tags: Vec<SentryTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRef {
    pub id: String,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub user_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryTag {
    pub key: String,
    pub values: Vec<String>,
}

/// A resolved + compact issue summary from the issues endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryIssueSummary {
    pub id: String,
    pub title: String,
    pub level: String,
    pub count: u32,
    pub user_count: u32,
    #[serde(rename = "lastSeen")]
    pub last_seen: String,
    pub permalink: String,
    #[serde(rename = "assignedTo")]
    pub assigned_to: Option<SentryUser>,
    pub status: String,
}

/// Event detail for digging into a specific occurrence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryEvent {
    pub id: String,
    #[serde(rename = "eventID")]
    pub event_id: String,
    pub level: String,
    pub message: Option<String>,
    #[serde(rename = "culprit")]
    pub culprit: String,
    #[serde(rename = "user")]
    pub user: Option<SentryUser>,
    #[serde(rename = "contexts")]
    pub contexts: Option<serde_json::Value>,
    #[serde(rename = "tags")]
    pub tags: Option<Vec<(String, String)>>,
    #[serde(rename = "exception")]
    pub exception: Option<ExceptionValue>,
    #[serde(rename = "timestamp")]
    pub timestamp: String,
    #[serde(rename = "received")]
    pub received: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionValue {
    pub values: Vec<ExceptionFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionFrame {
    #[serde(rename = "type")]
    pub type_: String,
    pub value: Option<String>,
    pub module: Option<String>,
    pub filename: Option<String>,
    pub function: Option<String>,
    pub lineno: Option<u32>,
    pub colno: Option<u32>,
    pub in_app: Option<bool>,
}

/// Response from POST /api/0/organizations/{org}/issues/{id}/actions/resolve/
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveResponse {
    pub status: String,
}

/// Stats for a project — used to detect spikes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStats {
    #[serde(rename = "ts")]
    pub timestamp: i64,
    #[serde(rename = "total")]
    pub total: f64,
    #[serde(rename = "filtered")]
    pub filtered: Option<f64>,
    #[serde(rename = "numEvents")]
    pub num_events: Option<f64>,
    #[serde(rename = "userCount")]
    pub user_count: Option<f64>,
}

/// Severity level of a Sentry issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityLevel {
    Fatal,
    Error,
    Warning,
    Info,
    Debug,
    Sample,
    Critical,
}

impl From<&str> for SeverityLevel {
    fn from(s: &str) -> Self {
        match s {
            "fatal" => SeverityLevel::Fatal,
            "warning" => SeverityLevel::Warning,
            "info" => SeverityLevel::Info,
            "debug" => SeverityLevel::Debug,
            "sample" => SeverityLevel::Sample,
            "critical" => SeverityLevel::Critical,
            _ => SeverityLevel::Error,
        }
    }
}

impl SeverityLevel {
    pub fn is_critical(self) -> bool {
        matches!(self, SeverityLevel::Fatal | SeverityLevel::Critical | SeverityLevel::Error)
    }
}
