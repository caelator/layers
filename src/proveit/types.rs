use chrono::{DateTime, Utc};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Top-level CLI arguments.
#[derive(Debug, clap::Parser)]
#[command(
    name = "proveit",
    version,
    about = "Executable proof gate for feature completion"
)]
pub struct Cli {
    /// Emit JSON output for machine consumers.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: CommandKind,
}

/// Supported CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum CommandKind {
    /// Run all proofs for a feature and persist fresh artifacts.
    Verify { feature: String },
    /// Run proofs for a feature and fail if the feature may not close.
    Enforce { feature: String },
    /// Report the current verdict for one feature or for all manifests.
    Report { feature: Option<String> },
    /// Run proofs for features impacted by the current worktree changes.
    VerifyImpacted,
    /// Show cached verdict summary without running proofs.
    Status {
        /// Print actionable guidance for each feature.
        #[arg(long)]
        verbose: bool,
        /// Emit JSON output (overrides table format).
        #[arg(long)]
        json: bool,
        /// Print warnings but always exit 0 (for use in hooks).
        #[arg(long)]
        warn_only: bool,
    },
}

/// Lower-case proof categories used for scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum ProofCategory {
    Positive,
    Counterfactual,
    Artifact,
    Failure,
    Repeatability,
}

impl ProofCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Counterfactual => "counterfactual",
            Self::Artifact => "artifact",
            Self::Failure => "failure",
            Self::Repeatability => "repeatability",
        }
    }
}

/// Artifact extraction strategy for proof stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactExtractMode {
    Json,
    LastLine,
    FullOutput,
}

/// Manifest model for a feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureManifest {
    pub feature: FeatureMetadata,
    #[serde(default)]
    pub proofs: Vec<ProofSpec>,
}

/// Feature metadata section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureMetadata {
    pub id: String,
    pub title: Option<String>,
    pub owner: String,
    pub pm_task_id: Option<String>,
    #[serde(default)]
    pub watch_paths: Vec<String>,
    pub required_score: u8,
}

/// One proof command for a feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofSpec {
    pub id: String,
    pub category: ProofCategory,
    pub description: String,
    pub command: String,
    pub timeout_secs: u64,
    pub artifact_extract: Option<ArtifactExtractMode>,
}

/// Stored artifact record for a proof execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofRecord {
    pub proof_id: String,
    pub feature_id: String,
    pub category: ProofCategory,
    pub command: String,
    pub passed: bool,
    pub exit_code: i32,
    pub commit_sha: String,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_hash: String,
    pub stderr_hash: String,
    pub artifact: Option<Value>,
    pub artifact_error: Option<String>,
}

/// One proof in a computed verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofOutcome {
    pub proof_id: String,
    pub category: ProofCategory,
    pub description: String,
    pub passed: bool,
    pub stale: bool,
    pub exit_code: Option<i32>,
    pub commit_sha: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub matched_changes: Vec<String>,
    pub artifact_present: bool,
    pub artifact_error: Option<String>,
}

/// Computed verdict for one feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureVerdict {
    pub feature_id: String,
    pub title: Option<String>,
    pub owner: String,
    pub pm_task_id: Option<String>,
    pub required_score: u8,
    pub score: u8,
    pub max_score: u8,
    pub may_close: bool,
    /// True when the feature has achieved the maximum score (all 5 categories
    /// present and passing) and is closable. Used by aggregate multi-feature
    /// commands (`report`, `verify-impacted`) to enforce a strict 5/5 gate: a
    /// full run is considered a failure unless every included feature is strict.
    #[serde(default)]
    pub strict: bool,
    pub stale: bool,
    pub missing_categories: Vec<ProofCategory>,
    pub changed_files: Vec<String>,
    pub watched_paths: Vec<String>,
    pub proofs: Vec<ProofOutcome>,
    pub recommended_gate_command: String,
}

/// Report payload for one or many features.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportOutput {
    pub workspace_root: String,
    pub features: Vec<FeatureVerdict>,
}
