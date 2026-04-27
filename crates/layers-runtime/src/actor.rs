//! Session actor: the glue between session management, mailbox queue, and agent loop.
//!
//! Each active session gets a `SessionActor` that owns its per-session queue,
//! coordinates message intake, runs the agent loop, and handles run-complete
//! drain cycles. The `SessionRuntime` manages the collection of active actors.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use layers_core::{
    InboundMessage, LayersError, Message, MessageContent, MessageRole, ModelProvider, Result,
    Session, SessionStore,
};

use crate::agent_loop::{self, RunConfig, RunStatus};
use crate::context::ContextAssembler;
use crate::failover::FailoverChain;
use crate::queue::{QueueMode, SessionQueue};
use crate::session::{SessionManager, SessionRouting};
use crate::streaming::{StreamEvent, StreamSink};
use crate::system_prompt::SystemPromptBuilder;
use crate::tool_dispatch::ToolRegistry;

// ---------------------------------------------------------------------------
// Actor command (sent to the per-session command loop)
// ---------------------------------------------------------------------------

/// Commands sent to a session actor's command loop.
enum ActorCommand {
    /// Submit a new inbound message for this session.
    Submit {
        message: InboundMessage,
        /// Optional stream sink for delivering events back to the caller.
        sink: Option<Arc<dyn StreamSink>>,
    },
    /// Request a manual reset of this session.
    Reset {
        /// Channel to send the new session back on.
        reply: mpsc::Sender<Result<Session>>,
    },
    /// Cancel the active run (if any).
    Cancel,
    /// Interrupt the active run and re-queue a new message.
    Interrupt { message: InboundMessage },
    /// Cron-triggered wakeup: run the given prompt in this session.
    CronWakeup { prompt: String },
    /// A subagent has completed; deliver its result back.
    SubagentComplete {
        subagent_session_id: String,
        result_summary: String,
    },
    /// Heartbeat tick.
    Heartbeat,
    /// Graceful shutdown.
    Shutdown,
}

// ---------------------------------------------------------------------------
// SessionActor
// ---------------------------------------------------------------------------

/// Owns the per-session command loop. The actor is driven by a `tokio::spawn`
/// that reads commands from `cmd_tx` and serialises all work for one session.
pub struct SessionActor {
    session_id: String,
    cmd_tx: mpsc::Sender<ActorCommand>,
    cancel: CancellationToken,
}

impl SessionActor {
    /// Spawn a new session actor. Returns the actor handle.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        session: Session,
        store: Arc<dyn SessionStore>,
        provider: Arc<dyn ModelProvider>,
        tools: Arc<ToolRegistry>,
        prompt_builder: Arc<SystemPromptBuilder>,
        context_assembler: Arc<ContextAssembler>,
        failover: Option<Arc<FailoverChain>>,
        run_config: RunConfig,
        queue_mode: QueueMode,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel(256);
        let cancel = CancellationToken::new();
        let (queue, steer_rx) = SessionQueue::with_steer_channel(queue_mode);

        let session_id = session.id.clone();
        let cancel_clone = cancel.clone();
        let queue = Arc::new(queue);

        let queue = queue; // Queue is used inside the actor loop.

        tokio::spawn(actor_loop(
            session,
            store,
            provider,
            tools,
            prompt_builder,
            context_assembler,
            failover,
            run_config,
            queue,
            cmd_rx,
            steer_rx,
            cancel_clone,
        ));

        Self {
            session_id,
            cmd_tx,
            cancel,
        }
    }

    /// Submit a message to this session's actor.
    pub async fn submit(
        &self,
        message: InboundMessage,
        sink: Option<Arc<dyn StreamSink>>,
    ) -> Result<()> {
        self.cmd_tx
            .send(ActorCommand::Submit { message, sink })
            .await
            .map_err(|_| LayersError::SessionClosed(self.session_id.clone()))
    }

    /// Request a reset of this session.
    pub async fn reset(&self) -> Result<Session> {
        let (reply_tx, mut reply_rx) = mpsc::channel(1);
        self.cmd_tx
            .send(ActorCommand::Reset { reply: reply_tx })
            .await
            .map_err(|_| LayersError::SessionClosed(self.session_id.clone()))?;
        reply_rx
            .recv()
            .await
            .ok_or_else(|| LayersError::SessionClosed(self.session_id.clone()))?
    }

    /// Cancel the active run.
    pub async fn cancel(&self) {
        self.cancel.cancel();
        let _ = self.cmd_tx.send(ActorCommand::Cancel).await;
    }

    /// Interrupt the active run and replace with a new message.
    pub async fn interrupt(&self, message: InboundMessage) -> Result<()> {
        self.cancel.cancel();
        self.cmd_tx
            .send(ActorCommand::Interrupt { message })
            .await
            .map_err(|_| LayersError::SessionCancelled(self.session_id.clone()))
    }

    /// Deliver a cron wakeup.
    pub async fn cron_wakeup(&self, prompt: String) -> Result<()> {
        self.cmd_tx
            .send(ActorCommand::CronWakeup { prompt })
            .await
            .map_err(|_| LayersError::SessionCancelled(self.session_id.clone()))
    }

    /// Deliver subagent completion.
    pub async fn subagent_complete(
        &self,
        subagent_session_id: String,
        result_summary: String,
    ) -> Result<()> {
        self.cmd_tx
            .send(ActorCommand::SubagentComplete {
                subagent_session_id,
                result_summary,
            })
            .await
            .map_err(|_| LayersError::SessionCancelled(self.session_id.clone()))
    }

    /// Deliver a heartbeat tick.
    pub async fn heartbeat(&self) -> Result<()> {
        self.cmd_tx
            .send(ActorCommand::Heartbeat)
            .await
            .map_err(|_| LayersError::SessionCancelled(self.session_id.clone()))
    }

    /// Shut down the actor gracefully.
    pub async fn shutdown(&self) -> Result<()> {
        self.cmd_tx
            .send(ActorCommand::Shutdown)
            .await
            .map_err(|_| LayersError::SessionCancelled(self.session_id.clone()))
    }

    /// Session ID of this actor.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    fn clone_handle(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            cmd_tx: self.cmd_tx.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Actor main loop (runs inside a tokio task)
// ---------------------------------------------------------------------------

/// The inner loop that processes commands for one session serially.
#[allow(clippy::too_many_arguments)]
#[allow(unused_variables, unused_assignments)]
async fn actor_loop(
    mut session: Session,
    store: Arc<dyn SessionStore>,
    provider: Arc<dyn ModelProvider>,
    tools: Arc<ToolRegistry>,
    prompt_builder: Arc<SystemPromptBuilder>,
    context_assembler: Arc<ContextAssembler>,
    failover: Option<Arc<FailoverChain>>,
    run_config: RunConfig,
    queue: Arc<SessionQueue>,
    mut cmd_rx: mpsc::Receiver<ActorCommand>,
    steer_rx: mpsc::Receiver<crate::queue::QueuedMessage>,
    #[allow(unused_assignments, unused_variables)] _run_cancel: CancellationToken,
) {
    let session_id = session.id.clone();
    debug!(session_id = %session_id, "session actor started");

    let mut active_run_cancel: Option<CancellationToken> = None;

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    None => {
                        info!(session_id = %session_id, "session actor shutting down (channel closed)");
                        if let Some(ref c) = active_run_cancel {
                            c.cancel();
                        }
                        break;
                    }
                    Some(ActorCommand::Cancel) => {
                        if let Some(ref c) = active_run_cancel {
                            c.cancel();
                            debug!(session_id = %session_id, "active run cancelled");
                        }
                    }
                    Some(ActorCommand::Reset { reply }) => {
                        if let Some(ref c) = active_run_cancel {
                            c.cancel();
                        }
                        active_run_cancel = None;

                        let new_session = {
                            let now = chrono::Utc::now();
                            let mut archived = session.clone();
                            archived.metadata.insert(
                                "archived_at".to_string(),
                                serde_json::Value::String(now.to_rfc3339()),
                            );
                            let _ = store.put(&archived).await;
                            let fresh = Session {
                                id: uuid::Uuid::new_v4().to_string(),
                                agent_id: session.agent_id.clone(),
                                dm_scope: session.dm_scope.clone(),
                                thread_binding: session.thread_binding.clone(),
                                created_at: now,
                                updated_at: now,
                                model: session.model.clone(),
                                metadata: HashMap::new(),
                                message_count: 0,
                                token_count: 0,
                            };
                            let _ = store.put(&fresh).await;
                            fresh
                        };
                        session = new_session;
                        let _ = reply.send(Ok(session.clone())).await;
                        info!(session_id = %session.id, "session reset complete");
                    }
                    Some(ActorCommand::Submit { message, sink }) => {
                        let user_msg = inbound_to_user_message(&message);
                        if let Err(e) = store.append_message(&session_id, user_msg.clone()).await {
                            error!(session_id = %session_id, error = %e, "failed to persist inbound message");
                            if let Some(sink) = sink {
                                sink.emit(StreamEvent::LifecycleEnd {
                                    session_id: session_id.clone(),
                                    status: RunStatus::Failed(format!("persist: {e}")),
                                }).await;
                            }
                            continue;
                        }

                        let run_cancel_token = CancellationToken::new();
                        active_run_cancel = Some(run_cancel_token.clone());

                        let result = agent_loop::run_agent_loop(
                            &session,
                            user_msg,
                            store.clone(),
                            provider.clone(),
                            tools.clone(),
                            &prompt_builder,
                            &context_assembler,
                            sink.clone(),
                            failover.as_ref().map(|f| f.as_ref()),
                            run_config.clone(),
                            run_cancel_token,
                        )
                        .await;

                        match result {
                            Ok(msgs) => {
                                debug!(session_id = %session_id, produced = msgs.len(), "agent run completed");
                            }
                            Err(LayersError::Cancelled) => {
                                debug!(session_id = %session_id, "agent run cancelled");
                            }
                            Err(e) => {
                                warn!(session_id = %session_id, error = %e, "agent run failed");
                            }
                        }

                        active_run_cancel = None;
                    }
                    Some(ActorCommand::Interrupt { message }) => {
                        // Cancel current run.
                        if let Some(ref c) = active_run_cancel {
                            c.cancel();
                        }
                        active_run_cancel = None;

                        // Persist and enqueue the interrupting message.
                        let user_msg = inbound_to_user_message(&message);
                        if let Err(e) = store.append_message(&session_id, user_msg.clone()).await {
                            error!(session_id = %session_id, error = %e, "failed to persist interrupt message");
                            continue;
                        }
                        let run_cancel_token = CancellationToken::new();
                        active_run_cancel = Some(run_cancel_token.clone());

                        let result = agent_loop::run_agent_loop(
                            &session,
                            user_msg,
                            store.clone(),
                            provider.clone(),
                            tools.clone(),
                            &prompt_builder,
                            &context_assembler,
                            None,
                            failover.as_ref().map(|f| f.as_ref()),
                            run_config.clone(),
                            run_cancel_token,
                        )
                        .await;

                        match result {
                            Ok(msgs) => {
                                debug!(session_id = %session_id, produced = msgs.len(), "interrupt run completed");
                            }
                            Err(LayersError::Cancelled) => {
                                debug!(session_id = %session_id, "interrupt run cancelled");
                            }
                            Err(e) => {
                                warn!(session_id = %session_id, error = %e, "interrupt run failed");
                            }
                        }
                        active_run_cancel = None;
                    }
                    Some(ActorCommand::CronWakeup { prompt }) => {
                        debug!(session_id = %session_id, "cron wakeup");
                        let inbound = make_system_inbound(&session, &prompt);
                        let user_msg = inbound_to_user_message(&inbound);
                        if let Err(e) = store.append_message(&session_id, user_msg.clone()).await {
                            error!(session_id = %session_id, error = %e, "failed to persist cron message");
                            continue;
                        }

                        let run_cancel_token = CancellationToken::new();
                        active_run_cancel = Some(run_cancel_token.clone());

                        let result = agent_loop::run_agent_loop(
                            &session,
                            user_msg,
                            store.clone(),
                            provider.clone(),
                            tools.clone(),
                            &prompt_builder,
                            &context_assembler,
                            None,
                            failover.as_ref().map(|f| f.as_ref()),
                            run_config.clone(),
                            run_cancel_token,
                        )
                        .await;

                        match result {
                            Ok(msgs) => {
                                debug!(session_id = %session_id, produced = msgs.len(), "cron run completed");
                            }
                            Err(LayersError::Cancelled) => {}
                            Err(e) => {
                                warn!(session_id = %session_id, error = %e, "cron run failed");
                            }
                        }
                        active_run_cancel = None;
                    }
                    Some(ActorCommand::SubagentComplete { subagent_session_id, result_summary }) => {
                        debug!(session_id = %session_id, subagent = %subagent_session_id, "subagent completed");
                        let text = format!("[subagent {} completed] {}", subagent_session_id, result_summary);
                        let inbound = make_system_inbound(&session, &text);
                        let user_msg = inbound_to_user_message(&inbound);
                        if let Err(e) = store.append_message(&session_id, user_msg.clone()).await {
                            error!(session_id = %session_id, error = %e, "failed to persist subagent result");
                            continue;
                        }

                        let run_cancel_token = CancellationToken::new();
                        active_run_cancel = Some(run_cancel_token.clone());

                        let result = agent_loop::run_agent_loop(
                            &session,
                            user_msg,
                            store.clone(),
                            provider.clone(),
                            tools.clone(),
                            &prompt_builder,
                            &context_assembler,
                            None,
                            failover.as_ref().map(|f| f.as_ref()),
                            run_config.clone(),
                            run_cancel_token,
                        )
                        .await;

                        match result {
                            Ok(msgs) => {
                                debug!(session_id = %session_id, produced = msgs.len(), "subagent-result run completed");
                            }
                            Err(LayersError::Cancelled) => {}
                            Err(e) => {
                                warn!(session_id = %session_id, error = %e, "subagent-result run failed");
                            }
                        }
                        active_run_cancel = None;
                    }
                    Some(ActorCommand::Heartbeat) => {
                        debug!(session_id = %session_id, "heartbeat tick");
                        let inbound = make_system_inbound(&session, "[heartbeat]");
                        let user_msg = inbound_to_user_message(&inbound);
                        if let Err(e) = store.append_message(&session_id, user_msg.clone()).await {
                            error!(session_id = %session_id, error = %e, "failed to persist heartbeat message");
                            continue;
                        }

                        let run_cancel_token = CancellationToken::new();
                        active_run_cancel = Some(run_cancel_token.clone());

                        let result = agent_loop::run_agent_loop(
                            &session,
                            user_msg,
                            store.clone(),
                            provider.clone(),
                            tools.clone(),
                            &prompt_builder,
                            &context_assembler,
                            None,
                            failover.as_ref().map(|f| f.as_ref()),
                            run_config.clone(),
                            run_cancel_token,
                        )
                        .await;

                        match result {
                            Ok(msgs) => {
                                debug!(session_id = %session_id, produced = msgs.len(), "heartbeat run completed");
                            }
                            Err(LayersError::Cancelled) => {}
                            Err(e) => {
                                warn!(session_id = %session_id, error = %e, "heartbeat run failed");
                            }
                        }
                        active_run_cancel = None;
                    }
                    Some(ActorCommand::Shutdown) => {
                        info!(session_id = %session_id, "session actor shutting down");
                        if let Some(ref c) = active_run_cancel {
                            c.cancel();
                        }
                        break;
                    }
                }
            }
        }
    }

    debug!(session_id = %session_id, "session actor exited");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Synthesize a system InboundMessage for internal events (cron, heartbeat, subagent).
fn make_system_inbound(session: &Session, text: &str) -> InboundMessage {
    InboundMessage {
        channel: session
            .dm_scope
            .as_ref()
            .map(|d| d.channel.clone())
            .unwrap_or_default(),
        channel_message_id: format!("sys-{}", uuid::Uuid::new_v4()),
        peer_id: "system".to_string(),
        peer_display_name: "System".to_string(),
        peer_kind: layers_core::PeerKind::System,
        text: text.to_string(),
        attachments: vec![],
        thread_id: session.thread_binding.as_ref().map(|t| t.thread_id.clone()),
        reply_to_message_id: None,
        channel_metadata: None,
        timestamp: chrono::Utc::now(),
    }
}

/// Convert an `InboundMessage` from a channel into a user `Message` for the store.
fn inbound_to_user_message(inbound: &InboundMessage) -> Message {
    let mut content_parts: Vec<layers_core::ContentPart> = vec![];

    if !inbound.text.is_empty() {
        content_parts.push(layers_core::ContentPart::Text {
            text: inbound.text.clone(),
        });
    }

    for att in &inbound.attachments {
        content_parts.push(layers_core::ContentPart::File {
            url: att.url.clone(),
            mime_type: if att.mime_type.is_empty() {
                None
            } else {
                Some(att.mime_type.clone())
            },
        });
    }

    Message {
        role: MessageRole::User,
        content: if content_parts.len() == 1 {
            match content_parts.pop() {
                Some(layers_core::ContentPart::Text { text }) => MessageContent::Text(text),
                Some(part) => MessageContent::Parts(vec![part]),
                None => MessageContent::Text(String::new()),
            }
        } else if content_parts.is_empty() {
            MessageContent::Text(String::new())
        } else {
            MessageContent::Parts(content_parts)
        },
        name: None,
        tool_calls: None,
        tool_call_id: None,
        reasoning: None,
        timestamp: Some(inbound.timestamp),
    }
}

// ---------------------------------------------------------------------------
// SessionRuntime — top-level manager for all session actors
// ---------------------------------------------------------------------------

/// The runtime that manages all active session actors.
///
/// This is the main entry point for the session subsystem. It owns the
/// `SessionManager` for routing/resolution and a map of active `SessionActor`
/// handles.
pub struct SessionRuntime {
    session_mgr: Arc<SessionManager>,
    store: Arc<dyn SessionStore>,
    provider: Arc<dyn ModelProvider>,
    tools: Arc<ToolRegistry>,
    prompt_builder: Arc<SystemPromptBuilder>,
    context_assembler: Arc<ContextAssembler>,
    failover: Option<Arc<FailoverChain>>,
    run_config: RunConfig,
    queue_mode: QueueMode,
    actors: RwLock<HashMap<String, SessionActor>>,
}

impl SessionRuntime {
    /// Create a new session runtime with all dependencies.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_mgr: SessionManager,
        store: Arc<dyn SessionStore>,
        provider: Arc<dyn ModelProvider>,
        tools: Arc<ToolRegistry>,
        prompt_builder: SystemPromptBuilder,
        context_assembler: ContextAssembler,
        failover: Option<FailoverChain>,
        run_config: RunConfig,
        queue_mode: QueueMode,
    ) -> Self {
        Self {
            session_mgr: Arc::new(session_mgr),
            store,
            provider,
            tools,
            prompt_builder: Arc::new(prompt_builder),
            context_assembler: Arc::new(context_assembler),
            failover: failover.map(Arc::new),
            run_config,
            queue_mode,
            actors: RwLock::new(HashMap::new()),
        }
    }

    /// Submit an inbound message. Resolves or creates the session, then
    /// delivers the message to its actor.
    pub async fn submit(
        &self,
        routing: &SessionRouting,
        message: InboundMessage,
        sink: Option<Arc<dyn StreamSink>>,
    ) -> Result<Session> {
        let session = self.session_mgr.resolve_session(routing).await?;
        let actor = self.get_or_spawn(&session).await?;
        actor.submit(message, sink).await?;
        Ok(session)
    }

    /// Reset a session by routing key.
    pub async fn reset_session(&self, routing: &SessionRouting) -> Result<Session> {
        let mut actors = self.actors.write().await;
        if let Some(actor) = actors.remove(&routing.session_key(self.session_mgr.dm_scope())) {
            actor.cancel().await;
        }
        drop(actors);
        self.session_mgr.manual_reset(routing).await
    }

    /// Get or spawn an actor for the given session.
    async fn get_or_spawn(&self, session: &Session) -> Result<SessionActor> {
        let session_id = session.id.clone();

        {
            let actors = self.actors.read().await;
            if let Some(actor) = actors.get(&session_id) {
                return Ok(actor.clone_handle());
            }
        }

        let mut actors = self.actors.write().await;
        if let Some(actor) = actors.get(&session_id) {
            return Ok(actor.clone_handle());
        }

        let actor = SessionActor::spawn(
            session.clone(),
            self.store.clone(),
            self.provider.clone(),
            self.tools.clone(),
            self.prompt_builder.clone(),
            self.context_assembler.clone(),
            self.failover.clone(),
            self.run_config.clone(),
            self.queue_mode,
        );

        actors.insert(session_id, actor);
        Ok(actors.values().last().unwrap().clone_handle())
    }

    /// Shut down all actors.
    pub async fn shutdown(&self) {
        let mut actors = self.actors.write().await;
        for (_, actor) in actors.drain() {
            let _ = actor.shutdown().await;
        }
    }

    /// Send a cron wakeup to a session's actor.
    pub async fn cron_wakeup(&self, session_id: &str, prompt: String) -> Result<()> {
        let actors = self.actors.read().await;
        if let Some(actor) = actors.get(session_id) {
            actor.cron_wakeup(prompt).await
        } else {
            Err(LayersError::SessionNotFound(session_id.to_string()))
        }
    }

    /// Deliver subagent completion to a session's actor.
    pub async fn subagent_complete(
        &self,
        session_id: &str,
        subagent_session_id: String,
        result_summary: String,
    ) -> Result<()> {
        let actors = self.actors.read().await;
        if let Some(actor) = actors.get(session_id) {
            actor
                .subagent_complete(subagent_session_id, result_summary)
                .await
        } else {
            Err(LayersError::SessionNotFound(session_id.to_string()))
        }
    }

    /// Deliver a heartbeat tick to a session's actor.
    pub async fn heartbeat(&self, session_id: &str) -> Result<()> {
        let actors = self.actors.read().await;
        if let Some(actor) = actors.get(session_id) {
            actor.heartbeat().await
        } else {
            Err(LayersError::SessionNotFound(session_id.to_string()))
        }
    }

    /// Interrupt a session's active run with a new message.
    pub async fn interrupt(&self, session_id: &str, message: InboundMessage) -> Result<()> {
        let actors = self.actors.read().await;
        if let Some(actor) = actors.get(session_id) {
            actor.interrupt(message).await
        } else {
            Err(LayersError::SessionNotFound(session_id.to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::DmScopeMode;

    #[test]
    fn inbound_to_user_message_simple_text() {
        let inbound = InboundMessage {
            channel: "webchat".into(),
            channel_message_id: "msg-1".into(),
            peer_id: "user-1".into(),
            peer_display_name: "Test User".into(),
            peer_kind: layers_core::PeerKind::User,
            text: "Hello, world!".into(),
            attachments: vec![],
            thread_id: None,
            reply_to_message_id: None,
            channel_metadata: None,
            timestamp: chrono::Utc::now(),
        };

        let msg = inbound_to_user_message(&inbound);
        assert_eq!(msg.role, MessageRole::User);
        match msg.content {
            MessageContent::Text(t) => assert_eq!(t, "Hello, world!"),
            _ => panic!("expected Text content"),
        }
    }

    #[test]
    fn inbound_to_user_message_with_attachment() {
        let inbound = InboundMessage {
            channel: "webchat".into(),
            channel_message_id: "msg-2".into(),
            peer_id: "user-1".into(),
            peer_display_name: "Test User".into(),
            peer_kind: layers_core::PeerKind::User,
            text: "Look at this".into(),
            attachments: vec![layers_core::MediaAttachment {
                url: "https://example.com/img.png".into(),
                mime_type: "image/png".into(),
                filename: None,
                size_bytes: None,
            }],
            thread_id: None,
            reply_to_message_id: None,
            channel_metadata: None,
            timestamp: chrono::Utc::now(),
        };

        let msg = inbound_to_user_message(&inbound);
        match msg.content {
            MessageContent::Parts(parts) => assert_eq!(parts.len(), 2),
            _ => panic!("expected Parts content with text + file"),
        }
    }

    #[test]
    fn inbound_to_user_message_empty() {
        let inbound = InboundMessage {
            channel: "webchat".into(),
            channel_message_id: "msg-3".into(),
            peer_id: "user-1".into(),
            peer_display_name: "Test User".into(),
            peer_kind: layers_core::PeerKind::User,
            text: String::new(),
            attachments: vec![],
            thread_id: None,
            reply_to_message_id: None,
            channel_metadata: None,
            timestamp: chrono::Utc::now(),
        };

        let msg = inbound_to_user_message(&inbound);
        match msg.content {
            MessageContent::Text(t) => assert!(t.is_empty()),
            _ => panic!("expected Text content"),
        }
    }

    #[test]
    fn session_routing_keys_differ_by_scope() {
        let routing = SessionRouting {
            agent_id: "agent-1".into(),
            channel: Some("telegram".into()),
            peer_id: Some("user-42".into()),
            account_id: Some("acct-1".into()),
            thread_id: None,
        };

        let main_key = routing.session_key(DmScopeMode::Main);
        let peer_key = routing.session_key(DmScopeMode::PerPeer);
        let ch_peer_key = routing.session_key(DmScopeMode::PerChannelPeer);

        assert_ne!(main_key, peer_key);
        assert_ne!(peer_key, ch_peer_key);
    }

    #[test]
    fn session_routing_includes_thread() {
        let routing = SessionRouting {
            agent_id: "agent-1".into(),
            channel: Some("discord".into()),
            peer_id: Some("user-42".into()),
            account_id: None,
            thread_id: Some("thread-99".into()),
        };

        let key = routing.session_key(DmScopeMode::PerChannelPeer);
        assert!(key.contains("thread:thread-99"));
    }
}
