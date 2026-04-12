//! Subagent spawning: isolated sub-sessions with parent-child cancellation.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::info;
use uuid::Uuid;

use layers_core::{
    LayersError, Message, MessageContent, MessageRole, ModelProvider,
    Result, Session, SessionStore,
};

use crate::agent_loop::{self, RunConfig};
use crate::context::ContextAssembler;
use crate::system_prompt::SystemPromptBuilder;
use crate::tool_dispatch::ToolRegistry;

// ---------------------------------------------------------------------------
// Subagent handle
// ---------------------------------------------------------------------------

/// Handle to a spawned subagent run.
#[derive(Debug)]
pub struct SubagentHandle {
    pub session_id: String,
    pub cancel: CancellationToken,
    pub join: tokio::task::JoinHandle<Result<Vec<Message>>>,
}

// ---------------------------------------------------------------------------
// Cleanup policy
// ---------------------------------------------------------------------------

/// What to do with a subagent session after completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum CleanupPolicy {
    /// Archive the session (mark as archived, keep messages).
    #[default]
    Archive,
    /// Keep the session as-is (can be resumed).
    Keep,
    /// Delete the session and its messages.
    Delete,
}


// ---------------------------------------------------------------------------
// Subagent manager
// ---------------------------------------------------------------------------

/// Manages subagent spawning with concurrency limits and parent-child cancellation.
pub struct SubagentManager {
    store: Arc<dyn SessionStore>,
    /// Maximum concurrent subagents.
    concurrency: Arc<Semaphore>,
    /// Active subagent handles keyed by session ID.
    active: Mutex<HashMap<String, CancellationToken>>,
    /// Default cleanup policy.
    cleanup_policy: CleanupPolicy,
}

impl SubagentManager {
    pub fn new(store: Arc<dyn SessionStore>, max_concurrent: usize) -> Self {
        Self {
            store,
            concurrency: Arc::new(Semaphore::new(max_concurrent)),
            active: Mutex::new(HashMap::new()),
            cleanup_policy: CleanupPolicy::Archive,
        }
    }

    pub fn with_cleanup(mut self, policy: CleanupPolicy) -> Self {
        self.cleanup_policy = policy;
        self
    }

    /// Spawn a subagent as an isolated session.
    ///
    /// The subagent gets its own session, cancel token (child of `parent_cancel`),
    /// and runs the agent loop independently.
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn(
        &self,
        parent_session: &Session,
        parent_cancel: &CancellationToken,
        prompt: String,
        provider: Arc<dyn ModelProvider>,
        tools: Arc<ToolRegistry>,
        prompt_builder: SystemPromptBuilder,
        config: RunConfig,
    ) -> Result<SubagentHandle> {
        // Acquire concurrency permit.
        let _permit = self
            .concurrency
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| LayersError::Provider("subagent concurrency limit closed".into()))?;

        // Create child cancel token.
        let child_cancel = parent_cancel.child_token();

        // Create isolated sub-session.
        let now = Utc::now();
        let sub_session = Session {
            id: Uuid::new_v4().to_string(),
            agent_id: parent_session.agent_id.clone(),
            dm_scope: None,
            thread_binding: None,
            created_at: now,
            updated_at: now,
            model: parent_session.model.clone(),
            metadata: {
                let mut meta = HashMap::new();
                meta.insert(
                    "parent_session".to_string(),
                    serde_json::Value::String(parent_session.id.clone()),
                );
                meta.insert(
                    "is_subagent".to_string(),
                    serde_json::Value::Bool(true),
                );
                meta
            },
            message_count: 0,
            token_count: 0,
        };

        self.store.put(&sub_session).await?;

        let session_id = sub_session.id.clone();
        let cancel_clone = child_cancel.clone();
        let store = Arc::clone(&self.store);
        let cleanup = self.cleanup_policy;

        // Track active subagent.
        self.active
            .lock()
            .await
            .insert(session_id.clone(), child_cancel.clone());

        let active_ref = self.active.lock().await;
        drop(active_ref);

        let store_for_cleanup = Arc::clone(&self.store);
        let sid_for_cleanup = session_id.clone();

        // Build the inbound message.
        let inbound = Message {
            role: MessageRole::User,
            content: MessageContent::Text(prompt),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: Some(now),
        };

        let context_assembler = ContextAssembler::new(provider.tokenizer());

        // Spawn the agent loop task.
        let join = tokio::spawn(async move {
            let result = agent_loop::run_agent_loop(
                &sub_session,
                inbound,
                store,
                provider,
                tools,
                &prompt_builder,
                &context_assembler,
                None, // No streaming for subagents by default.
                None, // No failover for subagents.
                config,
                cancel_clone,
            )
            .await;

            // Cleanup.
            match cleanup {
                CleanupPolicy::Archive => {
                    let mut session = sub_session;
                    session.metadata.insert(
                        "archived_at".to_string(),
                        serde_json::Value::String(Utc::now().to_rfc3339()),
                    );
                    let _ = store_for_cleanup.put(&session).await;
                }
                CleanupPolicy::Delete => {
                    let _ = store_for_cleanup.delete(&sid_for_cleanup).await;
                }
                CleanupPolicy::Keep => {}
            }

            // Drop permit when done (handled by _permit move).
            drop(_permit);
            result
        });

        info!(
            parent = %parent_session.id,
            subagent = %session_id,
            "spawned subagent"
        );

        Ok(SubagentHandle {
            session_id,
            cancel: child_cancel,
            join,
        })
    }

    /// Cancel all active subagents (e.g., when parent session is cancelled).
    pub async fn cancel_all(&self) {
        let active = self.active.lock().await;
        for (sid, cancel) in active.iter() {
            info!(subagent = %sid, "cancelling subagent");
            cancel.cancel();
        }
    }

    /// Number of currently active subagents.
    pub async fn active_count(&self) -> usize {
        self.active.lock().await.len()
    }
}
