//! The main agent loop: intake → context → inference → tool exec → persist → loop.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use layers_core::{
    LayersError, Message, MessageContent, MessageRole, ModelProvider, ModelRef,
    ModelRequest, Result, Session, SessionStore,
    ToolContext, TokenBudget,
};

use crate::context::ContextAssembler;
use crate::failover::FailoverChain;
use crate::streaming::{StreamEvent, StreamSink};
use crate::system_prompt::SystemPromptBuilder;
use crate::tool_dispatch::ToolRegistry;

// ---------------------------------------------------------------------------
// Agent run configuration
// ---------------------------------------------------------------------------

/// Configurable limits for an agent run.
#[derive(Debug, Clone)]
pub struct RunConfig {
    /// Maximum tool-loop iterations before aborting. Default: 50.
    pub max_tool_iterations: usize,
    /// Overall run timeout. Default: 48 hours.
    pub run_timeout: Duration,
    /// LLM idle timeout (time waiting for model response). Default: 60s.
    pub llm_idle_timeout: Duration,
    /// How many times the same (tool, args) pair can repeat. Default: 3.
    pub repeat_threshold: usize,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            max_tool_iterations: 50,
            run_timeout: Duration::from_secs(48 * 3600),
            llm_idle_timeout: Duration::from_secs(60),
            repeat_threshold: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Run status
// ---------------------------------------------------------------------------

/// Current status of an agent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    /// Waiting for input.
    Idle,
    /// Processing: running inference or tool execution.
    Running,
    /// Completed normally.
    Completed,
    /// Cancelled by user or parent.
    Cancelled,
    /// Failed with error.
    Failed(String),
}

// ---------------------------------------------------------------------------
// AgentRun — tracks a single run
// ---------------------------------------------------------------------------

/// Tracks the state of one serialized run within a session.
pub struct AgentRun {
    pub session_key: String,
    pub model_ref: ModelRef,
    pub cancel: CancellationToken,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub tool_iterations: usize,
    pub config: RunConfig,
}

impl AgentRun {
    pub fn new(session_key: String, model_ref: ModelRef, config: RunConfig) -> Self {
        Self {
            session_key,
            model_ref,
            cancel: CancellationToken::new(),
            status: RunStatus::Running,
            started_at: Utc::now(),
            tool_iterations: 0,
            config,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool-loop detection
// ---------------------------------------------------------------------------

/// Tracks recent tool calls for loop/repeat detection.
struct ToolLoopDetector {
    recent: Vec<(String, String)>, // (tool_name, args_hash)
    threshold: usize,
}

impl ToolLoopDetector {
    fn new(threshold: usize) -> Self {
        Self {
            recent: Vec::new(),
            threshold,
        }
    }

    fn record(&mut self, name: &str, args: &str) {
        self.recent.push((name.to_string(), args.to_string()));
    }

    /// Returns true if the same (tool, args) appeared `threshold` times consecutively.
    fn is_looping(&self) -> bool {
        if self.recent.len() < self.threshold {
            return false;
        }
        let last = &self.recent[self.recent.len() - 1];
        let tail = &self.recent[self.recent.len().saturating_sub(self.threshold)..];
        tail.iter().all(|entry| entry == last)
    }
}

// ---------------------------------------------------------------------------
// Agent loop entry point
// ---------------------------------------------------------------------------

/// Execute the main agent loop for a single inbound message within a session.
///
/// Returns the list of assistant messages produced (for streaming/queue callers).
pub async fn run_agent_loop(
    session: &Session,
    inbound: Message,
    store: Arc<dyn SessionStore>,
    provider: Arc<dyn ModelProvider>,
    tools: Arc<ToolRegistry>,
    prompt_builder: &SystemPromptBuilder,
    context_assembler: &ContextAssembler,
    stream_sink: Option<Arc<dyn StreamSink>>,
    failover: Option<&FailoverChain>,
    config: RunConfig,
    cancel: CancellationToken,
) -> Result<Vec<Message>> {
    let mut run = AgentRun::new(
        session.id.clone(),
        ModelRef {
            provider: provider.id().to_string(),
            model: session.model.clone().unwrap_or_default(),
        },
        config,
    );

    // Emit lifecycle:start.
    if let Some(ref sink) = stream_sink {
        sink.emit(StreamEvent::LifecycleStart {
            session_id: session.id.clone(),
        })
        .await;
    }

    // Persist the inbound user message.
    store.append_message(&session.id, inbound.clone()).await?;

    let mut produced: Vec<Message> = Vec::new();
    let mut loop_detector = ToolLoopDetector::new(run.config.repeat_threshold);

    // Build system prompt.
    let system_prompt = prompt_builder.build(session, &tools);

    // Build token budget.
    let budget = TokenBudget {
        max_input: provider.context_window().saturating_sub(provider.max_tokens()),
        max_output: provider.max_tokens(),
        reserved_for_tools: Some(4096),
    };

    // --- Main inference + tool loop ---
    loop {
        // Check cancellation.
        if cancel.is_cancelled() {
            run.status = RunStatus::Cancelled;
            break;
        }

        // Check tool iteration limit.
        if run.tool_iterations >= run.config.max_tool_iterations {
            warn!(
                session_id = %session.id,
                iterations = run.tool_iterations,
                "tool loop iteration limit reached"
            );
            run.status = RunStatus::Failed("tool loop iteration limit reached".into());
            break;
        }

        // Assemble context within budget.
        let messages = context_assembler
            .assemble(&session.id, &store, &budget, &system_prompt)
            .await?;

        // Build model request.
        let tool_defs = tools.definitions();
        let request = ModelRequest {
            model: run.model_ref.clone(),
            messages,
            system: Some(system_prompt.clone()),
            tools: if tool_defs.is_empty() { None } else { Some(tool_defs) },
            temperature: None,
            max_tokens: Some(budget.max_output),
            token_budget: Some(budget.clone()),
            thinking: None,
        };

        // Call model with timeout.
        let response = tokio::select! {
            _ = cancel.cancelled() => {
                run.status = RunStatus::Cancelled;
                break;
            }
            _ = tokio::time::sleep(run.config.llm_idle_timeout) => {
                // Try failover if available.
                if let Some(fo) = failover {
                    match fo.try_failover(request.clone(), &LayersError::Timeout(run.config.llm_idle_timeout)).await {
                        Ok(resp) => resp,
                        Err(e) => {
                            run.status = RunStatus::Failed(format!("LLM timeout + failover exhausted: {e}"));
                            break;
                        }
                    }
                } else {
                    run.status = RunStatus::Failed("LLM idle timeout".into());
                    break;
                }
            }
            result = provider.complete(request.clone()) => {
                match result {
                    Ok(resp) => resp,
                    Err(e) if is_failover_worthy(&e) => {
                        if let Some(fo) = failover {
                            match fo.try_failover(request, &e).await {
                                Ok(resp) => resp,
                                Err(fe) => {
                                    run.status = RunStatus::Failed(format!("{fe}"));
                                    break;
                                }
                            }
                        } else {
                            run.status = RunStatus::Failed(format!("{e}"));
                            break;
                        }
                    }
                    Err(e) => {
                        run.status = RunStatus::Failed(format!("{e}"));
                        break;
                    }
                }
            }
        };

        let assistant_msg = response.message.clone();

        // Stream text delta if present.
        if let Some(ref sink) = stream_sink {
            if let MessageContent::Text(ref text) = assistant_msg.content {
                sink.emit(StreamEvent::TextDelta(text.clone())).await;
            }
        }

        // Persist assistant message.
        store.append_message(&session.id, assistant_msg.clone()).await?;
        produced.push(assistant_msg.clone());

        // Check for tool calls.
        let tool_calls = match &assistant_msg.tool_calls {
            Some(tc) if !tc.is_empty() => tc.clone(),
            _ => {
                // No tool calls — run complete.
                run.status = RunStatus::Completed;
                break;
            }
        };

        // Execute tool calls.
        run.tool_iterations += 1;

        if let Some(ref sink) = stream_sink {
            for tc in &tool_calls {
                sink.emit(StreamEvent::ToolStart {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                })
                .await;
            }
        }

        for tc in &tool_calls {
            // Loop detection.
            loop_detector.record(&tc.function.name, &tc.function.arguments);
            if loop_detector.is_looping() {
                warn!(
                    tool = %tc.function.name,
                    "tool loop detected — same call repeated {} times",
                    run.config.repeat_threshold
                );
                run.status = RunStatus::Failed("tool loop detected".into());
                // Persist an error tool result so the model sees it.
                let err_msg = Message {
                    role: MessageRole::Tool,
                    content: MessageContent::Text(
                        "Error: tool loop detected — this call has been repeated too many times."
                            .into(),
                    ),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    reasoning: None,
                    timestamp: Some(Utc::now()),
                };
                store.append_message(&session.id, err_msg).await?;
                break;
            }

            let tool_ctx = ToolContext {
                session_id: session.id.clone(),
                agent_id: session.agent_id.clone(),
                channel: session.dm_scope.as_ref().map(|d| d.channel.clone()),
                metadata: session.metadata.clone(),
            };

            let args: serde_json::Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);

            let result = tools.dispatch(&tc.function.name, args, tool_ctx).await;

            let tool_msg = match result {
                Ok(output) => Message {
                    role: MessageRole::Tool,
                    content: MessageContent::Text(output.content),
                    name: Some(tc.function.name.clone()),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    reasoning: None,
                    timestamp: Some(Utc::now()),
                },
                Err(e) => Message {
                    role: MessageRole::Tool,
                    content: MessageContent::Text(format!("Error: {e}")),
                    name: Some(tc.function.name.clone()),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    reasoning: None,
                    timestamp: Some(Utc::now()),
                },
            };

            if let Some(ref sink) = stream_sink {
                sink.emit(StreamEvent::ToolEnd {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                })
                .await;
            }

            store.append_message(&session.id, tool_msg).await?;
        }

        // If loop was detected inside tool execution, break outer loop.
        if run.status != RunStatus::Running {
            break;
        }

        // Loop back to inference with tool results in context.
    }

    // Emit lifecycle:end.
    if let Some(ref sink) = stream_sink {
        sink.emit(StreamEvent::LifecycleEnd {
            session_id: session.id.clone(),
            status: run.status.clone(),
        })
        .await;
    }

    match &run.status {
        RunStatus::Failed(msg) => Err(LayersError::Provider(msg.clone())),
        RunStatus::Cancelled => Err(LayersError::Cancelled),
        _ => Ok(produced),
    }
}

/// Determine if an error should trigger model failover.
pub fn is_failover_worthy(err: &LayersError) -> bool {
    match err {
        LayersError::RateLimited { .. } | LayersError::Timeout(_) => true,
        LayersError::Provider(msg) => {
            msg.contains("overloaded")
                || msg.contains("billing")
                || msg.contains("auth")
                || msg.contains("rate limit")
        }
        _ => false,
    }
}
