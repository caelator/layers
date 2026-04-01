use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDecision {
    pub route: String,
    pub confidence: String,
    pub scores: serde_json::Map<String, Value>,
    pub matches: serde_json::Map<String, Value>,
    pub rationale: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub kind: String,
    pub score: Option<f64>,
    pub timestamp: Option<String>,
    pub task: Option<String>,
    pub summary: String,
    pub artifacts_dir: Option<String>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_context: Option<GraphContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitNexusIndexVersion {
    #[serde(default)]
    pub indexed_at: String,
    #[serde(default)]
    pub last_commit: String,
    #[serde(default)]
    pub stats: Value,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImplementationContext {
    #[serde(default)]
    pub target_symbols: Vec<String>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub affected_flows: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewContext {
    #[serde(default)]
    pub before_scope: Vec<String>,
    #[serde(default)]
    pub after_scope: Vec<String>,
    #[serde(default)]
    pub drift_symbols: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphContext {
    #[serde(default)]
    pub gitnexus_index_version: GitNexusIndexVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact_summary: Option<ImpactSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation_context: Option<ImplementationContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_context: Option<ReviewContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryBrief {
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub status: Vec<String>,
    #[serde(default)]
    pub next_steps: Vec<String>,
    #[serde(default)]
    pub postmortems: Vec<String>,
    #[serde(default)]
    pub notable_context: Vec<String>,
}

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
    pub graph_context: Option<GraphContext>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
}

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
pub struct StatusRecord {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub state: String,
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
    Project(Project),
    Task(Task),
    Decision(Decision),
    Constraint(Constraint),
    Status(StatusRecord),
    NextStep(NextStep),
    Postmortem(Postmortem),
}

impl ProjectRecordPayload {
    pub fn entity_name(&self) -> &'static str {
        match self {
            Self::Project(_) => "project",
            Self::Task(_) => "task",
            Self::Decision(_) => "decision",
            Self::Constraint(_) => "constraint",
            Self::Status(_) => "status",
            Self::NextStep(_) => "next_step",
            Self::Postmortem(_) => "postmortem",
        }
    }

    pub fn slug(&self) -> &str {
        match self {
            Self::Project(item) => &item.slug,
            Self::Task(item) => &item.slug,
            Self::Decision(item) => &item.slug,
            Self::Constraint(item) => &item.slug,
            Self::Status(item) => &item.slug,
            Self::NextStep(item) => &item.slug,
            Self::Postmortem(item) => &item.slug,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Project(item) => &item.title,
            Self::Task(item) => &item.title,
            Self::Decision(item) => &item.title,
            Self::Constraint(item) => &item.title,
            Self::Status(item) => &item.title,
            Self::NextStep(item) => &item.title,
            Self::Postmortem(item) => &item.title,
        }
    }

    pub fn summary(&self) -> &str {
        match self {
            Self::Project(item) => &item.summary,
            Self::Task(item) => &item.summary,
            Self::Decision(item) => &item.summary,
            Self::Constraint(item) => &item.summary,
            Self::Status(item) => &item.summary,
            Self::NextStep(item) => &item.summary,
            Self::Postmortem(item) => &item.summary,
        }
    }
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
