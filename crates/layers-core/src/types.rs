use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Vec<ReasoningPart>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningPart {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    ImageUrl {
        image_url: ImageUrl,
    },
    AudioUrl {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    VideoUrl {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    File {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool call / result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: ToolResultContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

// ---------------------------------------------------------------------------
// Session types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dm_scope: Option<DmScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_binding: Option<ThreadBinding>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    pub message_count: usize,
    pub token_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmScope {
    pub channel: String,
    pub peer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadBinding {
    pub channel: String,
    pub thread_id: String,
}

// ---------------------------------------------------------------------------
// Inbound / outbound message types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub channel: String,
    pub channel_message_id: String,
    pub peer_id: String,
    pub peer_display_name: String,
    pub peer_kind: PeerKind,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<MediaAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_metadata: Option<ChannelMetadata>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PeerKind {
    User,
    Bot,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAttachment {
    pub url: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMetadata {
    #[serde(default)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<MediaAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<StreamingMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamingMode {
    Disabled,
    EditInPlace,
    ChunkedMessages,
}

// ---------------------------------------------------------------------------
// Model types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

impl ModelRef {
    pub fn full_id(&self) -> String {
        format!("{}:{}", self.provider, self.model)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    pub model: ModelRef,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<TokenBudget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    pub max_input: usize,
    pub max_output: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved_for_tools: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<ThinkingLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    None,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    pub message: Message,
    pub usage: Usage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_delta: Option<ToolCallDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_delta: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool definition types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Agent types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub identity: AgentIdentity,
    pub model: ModelRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat: Option<HeartbeatConfig>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    pub interval: HumanDuration,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_hours: Option<ActiveHours>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveHours {
    pub start: String,
    pub end: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// A human-readable duration string (e.g. "30s", "5m", "1h").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanDuration(pub String);

// ---------------------------------------------------------------------------
// Cron types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub schedule: CronSchedule,
    pub payload: CronPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_target: Option<SessionTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<DeliveryConfig>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSchedule {
    pub cron: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronPayload {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTarget {
    New,
    Resume { session_id: String },
    Latest { filter: SessionFilter },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryConfig {
    pub mode: DeliveryMode,
    #[serde(default)]
    pub routes: Vec<DeliveryRoute>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_alert: Option<FailureAlert>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub misfire_policy: Option<MisFirePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    Silent,
    Notify,
    Stream,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryRoute {
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureAlert {
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mention: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MisFirePolicy {
    Skip,
    RunImmediately,
    Queue,
}

// ---------------------------------------------------------------------------
// Binding / matching types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<MatchRules>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchRules {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peers: Option<Vec<PeerMatch>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<PeerKind>,
}

// ---------------------------------------------------------------------------
// Runtime handle types
// ---------------------------------------------------------------------------

/// Handle for sending outbound messages from the orchestrator.
#[derive(Debug, Clone)]
pub struct OutboundHandle {
    pub channel: String,
    pub sender: mpsc::Sender<OutboundMessage>,
}

/// Target for streaming chunks to a channel.
#[derive(Debug, Clone)]
pub struct StreamingTarget {
    pub channel: String,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
}

/// Runtime handle for a connected channel adapter.
#[derive(Debug)]
pub struct ChannelRuntimeHandle {
    pub name: String,
    pub outbound: mpsc::Sender<OutboundMessage>,
    pub health: ChannelHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelHealth {
    Connected,
    Degraded,
    Disconnected,
}

// ---------------------------------------------------------------------------
// Tool context / output types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub session_id: String,
    pub agent_id: String,
    pub channel: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<MediaAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

// ---------------------------------------------------------------------------
// Auth profile types
// ---------------------------------------------------------------------------

/// A named auth profile for a model provider.
///
/// Stores credentials and configuration for connecting to a specific
/// provider (OpenAI, Anthropic, Google, etc.). Profiles are persisted
/// in the SQLite `auth_profiles` table and used to construct provider
/// instances at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProfile {
    /// Unique name for this profile (e.g. "openai-main", "anthropic-work").
    pub name: String,
    /// Provider identifier: "openai", "anthropic", "google", or a custom id.
    pub provider: String,
    /// API key (stored as-is; encryption is a future concern).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Optional custom API base URL (for OpenAI-compatible endpoints).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
    /// Optional list of model IDs this profile supports.
    #[serde(default)]
    pub models: Vec<String>,
    /// When this profile was created.
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// CronRun type
// ---------------------------------------------------------------------------

/// A record of a single cron job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRun {
    pub id: String,
    pub job_id: String,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub status: CronRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CronRunStatus {
    Running,
    Success,
    Failed,
    Skipped,
}

// ---------------------------------------------------------------------------
// Archive type
// ---------------------------------------------------------------------------

/// A snapshot of an archived session's messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub id: String,
    pub session_id: String,
    pub archived_at: DateTime<Utc>,
    pub message_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

// ---------------------------------------------------------------------------
// ProcessRun type
// ---------------------------------------------------------------------------

/// A record of a subagent / process execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessRun {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub status: ProcessRunStatus,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

// ---------------------------------------------------------------------------
// EmbeddingIndexState type
// ---------------------------------------------------------------------------

/// Tracks the indexing state of an embedding corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingIndexState {
    pub corpus: String,
    pub embedding_model: String,
    pub last_indexed_at: DateTime<Utc>,
    pub index_version: i64,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Context engine types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionResult {
    pub original_tokens: usize,
    pub compacted_tokens: usize,
    pub messages_removed: usize,
    pub messages_remaining: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,
}

/// Transaction handle for atomic session operations.
#[async_trait::async_trait]
pub trait SessionTransaction: Send + Sync {
    async fn append_message(&mut self, message: Message) -> crate::error::Result<()>;
    async fn update_session(&mut self, session: Session) -> crate::error::Result<()>;
    async fn commit(self: Box<Self>) -> crate::error::Result<()>;
}
