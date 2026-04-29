//! The main agent loop: intake → context → inference → tool exec → persist → loop.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use layers_core::{
    LayersError, Message, MessageContent, MessageRole, ModelProvider, ModelRef, ModelRequest,
    Result, Session, SessionStore, TokenBudget, ToolContext,
};

use crate::context::ContextAssembler;
use crate::failover::FailoverChain;
use crate::streaming::{StreamEvent, StreamSink};
use crate::system_prompt::SystemPromptBuilder;
use crate::tool_dispatch::{ToolCapabilityPolicy, ToolProfile, ToolRegistry};

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

const SESSION_TOOL_POLICY_METADATA_KEY: &str = "runtime_tool_policy";

fn session_tool_policy(
    session: &Session,
    base_policy: &ToolCapabilityPolicy,
) -> Option<ToolCapabilityPolicy> {
    let raw = session.metadata.get(SESSION_TOOL_POLICY_METADATA_KEY)?;
    let object = match raw.as_object() {
        Some(object) => object,
        None => {
            warn!(
                session_id = %session.id,
                metadata_key = SESSION_TOOL_POLICY_METADATA_KEY,
                "ignoring non-object runtime tool policy metadata"
            );
            return None;
        }
    };

    let mut policy = base_policy.clone();

    if let Some(profile) = object.get("profile") {
        let Some(profile_name) = profile.as_str() else {
            warn!(session_id = %session.id, "ignoring runtime tool policy profile that is not a string");
            return None;
        };

        match profile_name.parse::<ToolProfile>() {
            Ok(profile) => policy.profile = profile,
            Err(err) => {
                warn!(session_id = %session.id, error = %err, "ignoring invalid runtime tool policy profile");
                return None;
            }
        }
    }

    if let Some(allow) = parse_tool_name_list(session, object, "allow") {
        policy.allow = Some(allow);
    }
    if let Some(deny) = parse_tool_name_list(session, object, "deny") {
        policy.deny = deny;
    }

    Some(policy)
}

fn parse_tool_name_list(
    session: &Session,
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<Vec<String>> {
    let value = object.get(field)?;
    let items = match value.as_array() {
        Some(items) => items,
        None => {
            warn!(session_id = %session.id, field, "ignoring runtime tool policy field that is not an array");
            return None;
        }
    };

    let mut names = Vec::with_capacity(items.len());
    for item in items {
        let Some(name) = item.as_str() else {
            warn!(session_id = %session.id, field, "ignoring runtime tool policy field containing a non-string entry");
            return None;
        };
        names.push(name.to_string());
    }
    Some(names)
}

fn permitted_tool_names<'a>(
    tools: &'a ToolRegistry,
    policy: &ToolCapabilityPolicy,
) -> Vec<&'a str> {
    tools
        .names()
        .into_iter()
        .filter(|name| policy.allows(name))
        .collect()
}

fn permitted_tool_definitions(
    tools: &ToolRegistry,
    policy: &ToolCapabilityPolicy,
) -> Vec<layers_core::ToolDefinition> {
    tools
        .definitions()
        .into_iter()
        .filter(|definition| policy.allows(&definition.function.name))
        .collect()
}

// ---------------------------------------------------------------------------
// Agent loop entry point
// ---------------------------------------------------------------------------

/// Execute the main agent loop for a single inbound message within a session.
///
/// Returns the list of assistant messages produced (for streaming/queue callers).
#[allow(clippy::too_many_arguments)]
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
    let effective_tool_policy =
        session_tool_policy(session, tools.policy()).unwrap_or_else(|| tools.policy().clone());
    let permitted_tool_names = permitted_tool_names(&tools, &effective_tool_policy);

    // Build system prompt.
    let system_prompt = prompt_builder.build_with_tool_names(session, &permitted_tool_names);

    // Build token budget.
    let budget = TokenBudget {
        max_input: provider
            .context_window()
            .saturating_sub(provider.max_tokens()),
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
        let tool_defs = permitted_tool_definitions(&tools, &effective_tool_policy);
        let request = ModelRequest {
            model: run.model_ref.clone(),
            messages,
            system: Some(system_prompt.clone()),
            tools: if tool_defs.is_empty() {
                None
            } else {
                Some(tool_defs)
            },
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
        store
            .append_message(&session.id, assistant_msg.clone())
            .await?;
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

            let result = if effective_tool_policy.allows(&tc.function.name) {
                tools.dispatch(&tc.function.name, args, tool_ctx).await
            } else {
                Err(LayersError::Tool(format!(
                    "tool not permitted by session policy: {}",
                    tc.function.name
                )))
            };

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

#[cfg(test)]
mod tests {
    use super::{RunConfig, run_agent_loop};
    use crate::context::ContextAssembler;
    use crate::system_prompt::SystemPromptBuilder;
    use crate::tool_dispatch::ToolRegistry;
    use async_trait::async_trait;
    use chrono::Utc;
    use futures::stream;
    use layers_core::{
        FunctionCall, Message, MessageContent, MessageRole, ModelProvider, ModelRequest,
        ModelResponse, Result, Session, SessionFilter, SessionStore, SessionTransaction,
        StreamChunk, Tool, ToolCall, ToolContext, ToolOutput, Usage,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio_util::sync::CancellationToken;

    #[derive(Default)]
    struct TestStore {
        messages: Mutex<HashMap<String, Vec<Message>>>,
    }

    #[async_trait]
    impl SessionStore for TestStore {
        async fn get(&self, _session_id: &str) -> Result<Session> {
            unimplemented!()
        }

        async fn put(&self, _session: &Session) -> Result<()> {
            Ok(())
        }

        async fn list(&self, _filter: &SessionFilter) -> Result<Vec<Session>> {
            Ok(Vec::new())
        }

        async fn delete(&self, _session_id: &str) -> Result<()> {
            Ok(())
        }

        async fn append_message(&self, session_id: &str, message: Message) -> Result<()> {
            self.messages
                .lock()
                .expect("store lock")
                .entry(session_id.to_string())
                .or_default()
                .push(message);
            Ok(())
        }

        async fn get_messages(
            &self,
            session_id: &str,
            limit: Option<usize>,
        ) -> Result<Vec<Message>> {
            let messages = self
                .messages
                .lock()
                .expect("store lock")
                .get(session_id)
                .cloned()
                .unwrap_or_default();

            Ok(match limit {
                Some(limit) if messages.len() > limit => {
                    messages[messages.len() - limit..].to_vec()
                }
                _ => messages,
            })
        }

        async fn update_model(&self, _session_id: &str, _model: &str) -> Result<()> {
            Ok(())
        }

        async fn begin_session_tx(&self, _session_id: &str) -> Result<Box<dyn SessionTransaction>> {
            unimplemented!()
        }
    }

    struct RecordingProvider {
        responses: Mutex<Vec<ModelResponse>>,
        seen_tools: Mutex<Vec<Vec<String>>>,
    }

    #[async_trait]
    impl ModelProvider for RecordingProvider {
        fn id(&self) -> &str {
            "test-provider"
        }

        async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
            let tool_names = request
                .tools
                .unwrap_or_default()
                .into_iter()
                .map(|tool| tool.function.name)
                .collect::<Vec<_>>();
            self.seen_tools
                .lock()
                .expect("provider lock")
                .push(tool_names);
            Ok(self.responses.lock().expect("provider lock").remove(0))
        }

        fn complete_stream(
            &self,
            _request: ModelRequest,
        ) -> Pin<Box<dyn futures::Stream<Item = Result<StreamChunk>> + Send>> {
            Box::pin(stream::empty())
        }

        fn supports_tools(&self) -> bool {
            true
        }

        fn supports_vision(&self) -> bool {
            false
        }

        fn context_window(&self) -> usize {
            4096
        }

        fn max_tokens(&self) -> usize {
            512
        }

        fn tokenizer(&self) -> Option<Arc<dyn layers_core::Tokenizer>> {
            None
        }
    }

    struct CountingTool {
        name: &'static str,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Tool for CountingTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            "counting tool"
        }

        fn schema(&self) -> serde_json::Value {
            json!({"type": "object"})
        }

        async fn execute(
            &self,
            _args: serde_json::Value,
            _context: ToolContext,
        ) -> Result<ToolOutput> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput::text("ok"))
        }
    }

    fn message(role: MessageRole, text: &str) -> Message {
        Message {
            role,
            content: MessageContent::Text(text.to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: Some(Utc::now()),
        }
    }

    fn tool_call_message(name: &str) -> Message {
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text(format!("calling {name}")),
            name: None,
            tool_calls: Some(vec![ToolCall {
                id: "call-1".to_string(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: name.to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            tool_call_id: None,
            reasoning: None,
            timestamp: Some(Utc::now()),
        }
    }

    fn provider_with(messages: Vec<Message>) -> Arc<RecordingProvider> {
        Arc::new(RecordingProvider {
            responses: Mutex::new(
                messages
                    .into_iter()
                    .map(|message| ModelResponse {
                        message,
                        usage: Usage::default(),
                        model: Some("test-model".to_string()),
                        finish_reason: Some("stop".to_string()),
                    })
                    .collect(),
            ),
            seen_tools: Mutex::new(Vec::new()),
        })
    }

    fn session_with_policy(policy: serde_json::Value) -> Session {
        let mut metadata = HashMap::new();
        metadata.insert("runtime_tool_policy".to_string(), policy);
        Session {
            id: "s1".to_string(),
            agent_id: "agent".to_string(),
            dm_scope: None,
            thread_binding: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            model: Some("test-model".to_string()),
            metadata,
            message_count: 0,
            token_count: 0,
        }
    }

    fn registry_with_read_write(
        read_calls: Arc<AtomicUsize>,
        write_calls: Arc<AtomicUsize>,
    ) -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(CountingTool {
            name: "read",
            calls: read_calls,
        }));
        registry.register(Arc::new(CountingTool {
            name: "write",
            calls: write_calls,
        }));
        Arc::new(registry)
    }

    #[tokio::test]
    async fn session_tool_policy_filters_tools_sent_to_model() {
        let store: Arc<dyn SessionStore> = Arc::new(TestStore::default());
        let provider = provider_with(vec![message(MessageRole::Assistant, "done")]);
        let tools =
            registry_with_read_write(Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let prompt_builder = SystemPromptBuilder::new(None);
        let context_assembler = ContextAssembler::new(None);
        let session = session_with_policy(json!({"profile": "coding", "allow": ["read"]}));

        run_agent_loop(
            &session,
            message(MessageRole::User, "hi"),
            store,
            provider.clone(),
            tools,
            &prompt_builder,
            &context_assembler,
            None,
            None,
            RunConfig::default(),
            CancellationToken::new(),
        )
        .await
        .expect("agent loop should succeed");

        assert_eq!(
            provider.seen_tools.lock().expect("provider lock")[0],
            vec!["read".to_string()]
        );
    }

    #[tokio::test]
    async fn session_tool_policy_blocks_denied_dispatch() {
        let store_impl = Arc::new(TestStore::default());
        let store: Arc<dyn SessionStore> = store_impl.clone();
        let provider = provider_with(vec![
            tool_call_message("write"),
            message(MessageRole::Assistant, "done"),
        ]);
        let write_calls = Arc::new(AtomicUsize::new(0));
        let tools = registry_with_read_write(Arc::new(AtomicUsize::new(0)), write_calls.clone());
        let prompt_builder = SystemPromptBuilder::new(None);
        let context_assembler = ContextAssembler::new(None);
        let session = session_with_policy(json!({"profile": "coding", "allow": ["read"]}));

        run_agent_loop(
            &session,
            message(MessageRole::User, "hi"),
            store,
            provider,
            tools,
            &prompt_builder,
            &context_assembler,
            None,
            None,
            RunConfig::default(),
            CancellationToken::new(),
        )
        .await
        .expect("agent loop should succeed");

        assert_eq!(write_calls.load(Ordering::SeqCst), 0);
        let persisted = store_impl
            .messages
            .lock()
            .expect("store lock")
            .get("s1")
            .cloned()
            .unwrap_or_default();
        assert!(persisted.iter().any(|msg| {
            matches!(
                &msg.content,
                MessageContent::Text(text)
                    if text.contains("tool not permitted by session policy: write")
            )
        }));
    }
}
