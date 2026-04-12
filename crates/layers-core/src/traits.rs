use std::pin::Pin;

use chrono::{DateTime, Utc};
use std::sync::Arc;

use futures::Stream;

use crate::error::Result;
use crate::types::*;

/// Re-export CancellationToken for convenience.
pub type CancellationToken = tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// ChannelAdapter
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&self, cancel: CancellationToken) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn send(&self, message: OutboundMessage) -> Result<()>;
    async fn send_streaming(
        &self,
        target: StreamingTarget,
        chunk: String,
    ) -> Result<()>;
    async fn send_reaction(
        &self,
        channel: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<()>;
    async fn health(&self) -> ChannelHealth;
}

// ---------------------------------------------------------------------------
// ModelProvider
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &str;
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse>;
    fn complete_stream(
        &self,
        request: ModelRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>;
    fn supports_tools(&self) -> bool;
    fn supports_vision(&self) -> bool;
    fn context_window(&self) -> usize;
    fn max_tokens(&self) -> usize;
    fn tokenizer(&self) -> Option<Arc<dyn Tokenizer>>;
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

pub trait Tokenizer: Send + Sync {
    fn count_message_tokens(&self, messages: &[Message]) -> usize;
    fn count_tool_schema_tokens(&self, tools: &[ToolDefinition]) -> usize;
    fn count_text_tokens(&self, text: &str) -> usize;
}

// ---------------------------------------------------------------------------
// Tool
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;
    async fn execute(
        &self,
        args: serde_json::Value,
        context: ToolContext,
    ) -> Result<ToolOutput>;
}

// ---------------------------------------------------------------------------
// AuthProfileStore
// ---------------------------------------------------------------------------

/// Persistence interface for auth profiles.
#[async_trait::async_trait]
pub trait AuthProfileStore: Send + Sync {
    /// Insert or replace an auth profile.
    async fn put_profile(&self, profile: AuthProfile) -> Result<()>;
    /// Get a profile by name.
    async fn get_profile(&self, name: &str) -> Result<AuthProfile>;
    /// List all profiles, optionally filtered by provider.
    async fn list_profiles(&self, provider: Option<&str>) -> Result<Vec<AuthProfile>>;
    /// Delete a profile by name.
    async fn delete_profile(&self, name: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// CronStore
// ---------------------------------------------------------------------------

/// Persistence interface for cron jobs and their runs.
#[async_trait::async_trait]
pub trait CronStore: Send + Sync {
    async fn put_job(&self, job: CronJob) -> Result<()>;
    async fn get_job(&self, id: &str) -> Result<CronJob>;
    async fn list_jobs(&self) -> Result<Vec<CronJob>>;
    async fn delete_job(&self, id: &str) -> Result<()>;
    async fn put_run(&self, run: CronRun) -> Result<()>;
    async fn get_run(&self, id: &str) -> Result<CronRun>;
    async fn list_runs_for_job(&self, job_id: &str, limit: Option<usize>) -> Result<Vec<CronRun>>;
    async fn update_run_status(
        &self,
        id: &str,
        status: CronRunStatus,
        finished_at: DateTime<Utc>,
        error_message: Option<&str>,
    ) -> Result<()>;
}

// ---------------------------------------------------------------------------
// ArchiveStore
// ---------------------------------------------------------------------------

/// Persistence interface for session archives.
#[async_trait::async_trait]
pub trait ArchiveStore: Send + Sync {
    async fn put(&self, archive: Archive) -> Result<()>;
    async fn get(&self, id: &str) -> Result<Archive>;
    async fn list_for_session(&self, session_id: &str) -> Result<Vec<Archive>>;
    async fn delete(&self, id: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// ProcessRunStore
// ---------------------------------------------------------------------------

/// Persistence interface for process/subagent runs.
#[async_trait::async_trait]
pub trait ProcessRunStore: Send + Sync {
    async fn put(&self, run: ProcessRun) -> Result<()>;
    async fn get(&self, id: &str) -> Result<ProcessRun>;
    async fn list_by_parent(&self, parent_session_id: &str) -> Result<Vec<ProcessRun>>;
    async fn update_status(
        &self,
        id: &str,
        status: ProcessRunStatus,
        finished_at: DateTime<Utc>,
        result_summary: Option<&str>,
    ) -> Result<()>;
}

// ---------------------------------------------------------------------------
// EmbeddingIndexStore
// ---------------------------------------------------------------------------

/// Persistence interface for embedding index state tracking.
#[async_trait::async_trait]
pub trait EmbeddingIndexStore: Send + Sync {
    async fn put(&self, state: EmbeddingIndexState) -> Result<()>;
    async fn get(&self, corpus: &str) -> Result<EmbeddingIndexState>;
}



// ---------------------------------------------------------------------------
// ContextEngine
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait ContextEngine: Send + Sync {
    async fn ingest(&self, session_id: &str, message: &Message) -> Result<()>;
    async fn assemble(
        &self,
        session_id: &str,
        budget: &TokenBudget,
    ) -> Result<Vec<Message>>;
    async fn compact(&self, session_id: &str) -> Result<CompactionResult>;
    async fn prune(&self, session_id: &str, max_messages: usize) -> Result<()>;
}

// ---------------------------------------------------------------------------
// SessionStore
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, session_id: &str) -> Result<Session>;
    async fn put(&self, session: &Session) -> Result<()>;
    async fn list(&self, filter: &SessionFilter) -> Result<Vec<Session>>;
    async fn delete(&self, session_id: &str) -> Result<()>;
    async fn append_message(
        &self,
        session_id: &str,
        message: Message,
    ) -> Result<()>;
    async fn get_messages(
        &self,
        session_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Message>>;
    async fn update_model(
        &self,
        session_id: &str,
        model: &str,
    ) -> Result<()>;
    async fn begin_session_tx(
        &self,
        session_id: &str,
    ) -> Result<Box<dyn SessionTransaction>>;
}
