use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Graph context (passed to council prompts when --targets is specified)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlastRadius {
    #[serde(default)]
    pub direct: u64,
    #[serde(default)]
    pub indirect: u64,
    #[serde(default)]
    pub transitive: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImpactSummary {
    #[serde(default)]
    pub target_symbols: Vec<String>,
    #[serde(default)]
    pub blast_radius: BlastRadius,
    #[serde(default)]
    pub risk_level: String,
    #[serde(default)]
    pub affected_processes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Project and task management types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMeta {
    pub id: String,
    pub project: String,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Council execution types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CouncilStageAttempt {
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout_path: String,
    #[serde(default)]
    pub stderr_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CouncilStageRecord {
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub prompt_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default)]
    pub attempts: Vec<CouncilStageAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CouncilConvergenceRecord {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub decision: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub why: Vec<String>,
    #[serde(default)]
    pub unresolved: Vec<String>,
    #[serde(default)]
    pub next_steps: Vec<String>,
    #[serde(default)]
    pub missing_sections: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CouncilRunRecord {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_reason: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub workspace_root: String,
    #[serde(default)]
    pub artifacts_dir: String,
    #[serde(default)]
    pub route: String,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_context: Option<ImpactSummary>,
    #[serde(default)]
    pub context_text_path: String,
    #[serde(default)]
    pub context_json_path: String,
    #[serde(default)]
    pub retry_limit: u32,
    #[serde(default)]
    pub timeout_secs: u64,
    #[serde(default)]
    pub degraded_reasons: Vec<String>,
    #[serde(default)]
    pub artifact_errors: Vec<String>,
    #[serde(default)]
    pub stages: Vec<CouncilStageRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convergence: Option<CouncilConvergenceRecord>,
}

// ---------------------------------------------------------------------------
// Curated memory record types (used by council promote and curated import)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub impact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextStep {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Postmortem {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub root_cause: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProjectRecordPayload {
    Decision(Decision),
    Constraint(Constraint),
    NextStep(NextStep),
    Postmortem(Postmortem),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: String,
    pub entity: String,
    pub project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub created_at: String,
    pub source: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    pub payload: ProjectRecordPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuratedImportRecord {
    pub kind: String,
    pub project: String,
    pub summary: String,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}
