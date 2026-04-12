//! Token accounting and budget enforcement.
//!
//! Provides a [`TokenAccountant`] that tracks cumulative token usage per session
//! and enforces budgets before provider calls. This is the internal "hooks" layer
//! that sits between the runtime and the provider — every request passes through
//! budget validation, and every response records usage.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, warn};

use layers_core::error::{LayersError, Result};
use layers_core::traits::{ModelProvider, Tokenizer};
use layers_core::types::*;

// ---------------------------------------------------------------------------
// Usage snapshot
// ---------------------------------------------------------------------------

/// Cumulative token usage for a single session.
#[derive(Debug, Clone, Default)]
pub struct UsageSnapshot {
    /// Total prompt tokens consumed across all requests.
    pub prompt_tokens: usize,
    /// Total completion tokens consumed across all requests.
    pub completion_tokens: usize,
    /// Total reasoning tokens (extended thinking) consumed.
    pub reasoning_tokens: usize,
    /// Total cache-read tokens (prompt caching savings).
    pub cache_read_tokens: usize,
    /// Number of provider calls recorded.
    pub request_count: usize,
}

impl UsageSnapshot {
    /// Merge a single-request `Usage` into this snapshot.
    pub fn record(&mut self, usage: &Usage) {
        self.prompt_tokens += usage.prompt_tokens;
        self.completion_tokens += usage.completion_tokens;
        self.reasoning_tokens += usage.reasoning_tokens.unwrap_or(0);
        self.cache_read_tokens += usage.cache_read_tokens.unwrap_or(0);
        self.request_count += 1;
    }

    /// Total tokens (prompt + completion + reasoning).
    pub fn total(&self) -> usize {
        self.prompt_tokens + self.completion_tokens + self.reasoning_tokens
    }
}

// ---------------------------------------------------------------------------
// Accounting key
// ---------------------------------------------------------------------------

/// Key for looking up usage by session and agent.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct AccountingKey {
    pub session_id: String,
    pub agent_id: String,
}

impl AccountingKey {
    pub fn new(session_id: impl Into<String>, agent_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            agent_id: agent_id.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Budget check result
// ---------------------------------------------------------------------------

/// Result of a pre-flight budget check.
#[derive(Debug, Clone)]
pub struct BudgetCheck {
    /// Estimated input tokens for the pending request.
    pub estimated_input: usize,
    /// Remaining input budget after this request.
    pub remaining_input: usize,
    /// Whether the request fits within the budget.
    pub within_budget: bool,
}

// ---------------------------------------------------------------------------
// TokenAccountant
// ---------------------------------------------------------------------------

/// Thread-safe token accounting tracker.
///
/// Tracks cumulative usage per `(session, agent)` pair and provides budget
/// enforcement hooks. Designed to be shared via `Arc` across the runtime.
pub struct TokenAccountant {
    usage: RwLock<HashMap<AccountingKey, UsageSnapshot>>,
}

impl TokenAccountant {
    /// Create a new empty accountant.
    pub fn new() -> Self {
        Self {
            usage: RwLock::new(HashMap::new()),
        }
    }

    /// Record usage from a completed provider call.
    pub fn record_usage(&self, key: &AccountingKey, usage: &Usage) {
        let mut map = self.usage.write();
        map.entry(key.clone())
            .or_default()
            .record(usage);

        debug!(
            session = %key.session_id,
            agent = %key.agent_id,
            prompt = usage.prompt_tokens,
            completion = usage.completion_tokens,
            cumulative = map[key].total(),
            "recorded token usage"
        );
    }

    /// Get the current usage snapshot for a key.
    pub fn get_usage(&self, key: &AccountingKey) -> UsageSnapshot {
        self.usage
            .read()
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    /// Reset usage for a key (e.g., on session reset).
    pub fn reset(&self, key: &AccountingKey) {
        if let Some(snapshot) = self.usage.write().remove(key) {
            debug!(
                session = %key.session_id,
                agent = %key.agent_id,
                total = snapshot.total(),
                "reset token accounting"
            );
        }
    }

    /// Estimate input tokens for a request using the provider's tokenizer.
    ///
    /// Counts message tokens, tool schema tokens, and system prompt tokens.
    pub fn estimate_input(
        provider: &dyn ModelProvider,
        request: &ModelRequest,
    ) -> usize {
        if let Some(tokenizer) = provider.tokenizer() {
            let msg_tokens = tokenizer.count_message_tokens(&request.messages);
            let tool_tokens = request
                .tools
                .as_ref()
                .map(|t| tokenizer.count_tool_schema_tokens(t))
                .unwrap_or(0);
            let system_tokens = request
                .system
                .as_ref()
                .map(|s| tokenizer.count_text_tokens(s))
                .unwrap_or(0);
            msg_tokens + tool_tokens + system_tokens
        } else {
            // Fallback: rough character-based estimate (~4 chars/token)
            let msg_chars: usize = request
                .messages
                .iter()
                .map(|m| match &m.content {
                    MessageContent::Text(t) => t.len(),
                    MessageContent::Parts(_) => 100,
                })
                .sum();
            let sys_chars = request.system.as_ref().map(|s| s.len()).unwrap_or(0);
            (msg_chars + sys_chars) / 4
        }
    }

    /// Check whether a request fits within the budget.
    ///
    /// If `TokenBudget` is set on the request, checks estimated input tokens
    /// against `max_input` minus cumulative prompt tokens for the session.
    /// Returns a `BudgetCheck` with the outcome.
    pub fn check_budget(
        &self,
        key: &AccountingKey,
        provider: &dyn ModelProvider,
        request: &ModelRequest,
    ) -> Result<BudgetCheck> {
        let estimated_input = Self::estimate_input(provider, request);

        let budget = match &request.token_budget {
            Some(b) => b,
            None => {
                // No budget configured — always allow.
                return Ok(BudgetCheck {
                    estimated_input,
                    remaining_input: usize::MAX,
                    within_budget: true,
                });
            }
        };

        let snapshot = self.get_usage(key);
        let used = snapshot.prompt_tokens;
        let remaining = budget.max_input.saturating_sub(used);

        let within_budget = estimated_input <= remaining;

        if !within_budget {
            warn!(
                session = %key.session_id,
                estimated = estimated_input,
                budget = budget.max_input,
                used,
                remaining,
                "token budget exceeded — compaction may be needed"
            );
        }

        Ok(BudgetCheck {
            estimated_input,
            remaining_input: remaining,
            within_budget,
        })
    }

    /// Pre-flight hook: estimate input and return `ContextOverflow` error
    /// if the request exceeds the provider's context window.
    ///
    /// This is the hard boundary — even without a `TokenBudget`, the request
    /// must fit within the model's context window.
    pub fn validate_context_window(
        provider: &dyn ModelProvider,
        request: &ModelRequest,
    ) -> Result<()> {
        let estimated = Self::estimate_input(provider, request);
        let window = provider.context_window();

        if estimated > window {
            return Err(LayersError::ContextOverflow {
                used: estimated,
                limit: window,
            });
        }

        Ok(())
    }
}

impl Default for TokenAccountant {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// AccountedProvider — decorator that wraps any ModelProvider with accounting
// ---------------------------------------------------------------------------

/// A decorator that wraps a [`ModelProvider`] and automatically records token
/// usage through a shared [`TokenAccountant`].
///
/// Usage:
/// ```ignore
/// let accountant = Arc::new(TokenAccountant::new());
/// let accounted = AccountedProvider::new(openai_provider, accountant.clone(), "agent:main".into());
/// // Use accounted.complete() as normal — usage is auto-recorded.
/// ```
pub struct AccountedProvider {
    inner: Box<dyn ModelProvider>,
    accountant: Arc<TokenAccountant>,
    accounting_key: AccountingKey,
}

impl AccountedProvider {
    /// Create a new accounted wrapper.
    pub fn new(
        inner: Box<dyn ModelProvider>,
        accountant: Arc<TokenAccountant>,
        accounting_key: AccountingKey,
    ) -> Self {
        Self {
            inner,
            accountant,
            accounting_key,
        }
    }
}

#[async_trait::async_trait]
impl ModelProvider for AccountedProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        // Pre-flight: validate context window
        TokenAccountant::validate_context_window(self.inner.as_ref(), &request)?;

        // Optional: check budget
        let check = self.accountant.check_budget(
            &self.accounting_key,
            self.inner.as_ref(),
            &request,
        )?;

        if !check.within_budget {
            return Err(LayersError::ContextOverflow {
                used: check.estimated_input,
                limit: check.remaining_input + check.estimated_input,
            });
        }

        let response = self.inner.complete(request).await?;

        // Post-flight: record usage
        self.accountant
            .record_usage(&self.accounting_key, &response.usage);

        Ok(response)
    }

    fn complete_stream(
        &self,
        request: ModelRequest,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = Result<StreamChunk>> + Send>> {
        // Pre-flight: validate context window
        if let Err(e) = TokenAccountant::validate_context_window(self.inner.as_ref(), &request) {
            return Box::pin(futures::stream::once(async move { Err(e) }));
        }

        // For streaming, we delegate — usage recording happens when the stream
        // yields the final chunk with usage data. The runtime is responsible for
        // calling `accountant.record_usage()` when the stream completes.
        self.inner.complete_stream(request)
    }

    fn supports_tools(&self) -> bool {
        self.inner.supports_tools()
    }

    fn supports_vision(&self) -> bool {
        self.inner.supports_vision()
    }

    fn context_window(&self) -> usize {
        self.inner.context_window()
    }

    fn max_tokens(&self) -> usize {
        self.inner.max_tokens()
    }

    fn tokenizer(&self) -> Option<std::sync::Arc<dyn Tokenizer>> {
        self.inner.tokenizer()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_usage(prompt: usize, completion: usize) -> Usage {
        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
        }
    }

    #[test]
    fn snapshot_records_usage() {
        let mut snap = UsageSnapshot::default();
        snap.record(&make_usage(100, 50));
        snap.record(&make_usage(200, 75));

        assert_eq!(snap.prompt_tokens, 300);
        assert_eq!(snap.completion_tokens, 125);
        assert_eq!(snap.request_count, 2);
        assert_eq!(snap.total(), 425);
    }

    #[test]
    fn snapshot_tracks_reasoning_tokens() {
        let mut snap = UsageSnapshot::default();
        let mut usage = make_usage(100, 50);
        usage.reasoning_tokens = Some(30);
        snap.record(&usage);

        assert_eq!(snap.reasoning_tokens, 30);
        assert_eq!(snap.total(), 180);
    }

    #[test]
    fn accountant_records_and_gets() {
        let acc = TokenAccountant::new();
        let key = AccountingKey::new("s1", "main");

        acc.record_usage(&key, &make_usage(500, 200));

        let snap = acc.get_usage(&key);
        assert_eq!(snap.prompt_tokens, 500);
        assert_eq!(snap.completion_tokens, 200);
        assert_eq!(snap.request_count, 1);
    }

    #[test]
    fn accountant_resets_key() {
        let acc = TokenAccountant::new();
        let key = AccountingKey::new("s1", "main");

        acc.record_usage(&key, &make_usage(500, 200));
        acc.reset(&key);

        let snap = acc.get_usage(&key);
        assert_eq!(snap.request_count, 0);
    }

    #[test]
    fn accountant_default_is_empty() {
        let acc = TokenAccountant::new();
        let key = AccountingKey::new("nonexistent", "agent");
        let snap = acc.get_usage(&key);
        assert_eq!(snap.total(), 0);
    }

    #[test]
    fn budget_check_allows_when_within() {
        let acc = TokenAccountant::new();
        let key = AccountingKey::new("s1", "main");

        // No budget set — always allowed
        let request = ModelRequest {
            model: ModelRef { provider: "test".into(), model: "test".into() },
            messages: vec![],
            system: Some("hello world".into()),
            tools: None,
            temperature: None,
            max_tokens: None,
            token_budget: Some(TokenBudget {
                max_input: 1000,
                max_output: 500,
                reserved_for_tools: None,
            }),
            thinking: None,
        };

        // Use the OpenAI provider's tokenizer for estimation
        let provider = crate::openai::OpenAiProvider::new("test", "http://localhost", "key");
        let check = acc.check_budget(&key, &provider, &request).unwrap();
        assert!(check.within_budget);
        // "hello world" is ~2 tokens with tiktoken, well under 1000
    }

    #[test]
    fn budget_check_rejects_when_exceeded() {
        let acc = TokenAccountant::new();
        let key = AccountingKey::new("s1", "main");

        // Pre-record enough usage to fill the budget
        let mut big_usage = make_usage(950, 0);
        big_usage.cache_read_tokens = Some(0);
        acc.record_usage(&key, &big_usage);

        let request = ModelRequest {
            model: ModelRef { provider: "test".into(), model: "test".into() },
            messages: vec![Message {
                role: MessageRole::User,
                content: MessageContent::Text("a".repeat(400)), // 100 tokens approx
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning: None,
                timestamp: None,
            }],
            system: None,
            tools: None,
            temperature: None,
            max_tokens: None,
            token_budget: Some(TokenBudget {
                max_input: 1000,
                max_output: 500,
                reserved_for_tools: None,
            }),
            thinking: None,
        };

        let provider = crate::openai::OpenAiProvider::new("test", "http://localhost", "key");
        let check = acc.check_budget(&key, &provider, &request).unwrap();
        // 950 used + estimated input > 1000 budget
        assert!(!check.within_budget);
    }

    #[test]
    fn no_budget_always_allows() {
        let acc = TokenAccountant::new();
        let key = AccountingKey::new("s1", "main");

        let request = ModelRequest {
            model: ModelRef { provider: "test".into(), model: "test".into() },
            messages: vec![],
            system: None,
            tools: None,
            temperature: None,
            max_tokens: None,
            token_budget: None, // No budget
            thinking: None,
        };

        let provider = crate::openai::OpenAiProvider::new("test", "http://localhost", "key");
        let check = acc.check_budget(&key, &provider, &request).unwrap();
        assert!(check.within_budget);
    }

    #[test]
    fn context_window_validation_rejects_oversized() {
        let provider = crate::openai::OpenAiProvider::new("test", "http://localhost", "key");
        // context_window() returns 128_000 for OpenAI

        // tiktoken o200k_base: need enough text to exceed 128k tokens
        // ~4 chars/token, so we need >512k chars of varied text
        let big_text = "The quick brown fox jumps over the lazy dog. ".repeat(20_000); // ~920k chars
        assert!(big_text.len() > 500_000);
        let request = ModelRequest {
            model: ModelRef { provider: "test".into(), model: "test".into() },
            messages: vec![Message {
                role: MessageRole::User,
                content: MessageContent::Text(big_text),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning: None,
                timestamp: None,
            }],
            system: None,
            tools: None,
            temperature: None,
            max_tokens: None,
            token_budget: None,
            thinking: None,
        };

        let result = TokenAccountant::validate_context_window(&provider, &request);
        assert!(result.is_err());
        match result.unwrap_err() {
            LayersError::ContextOverflow { .. } => {}
            other => panic!("expected ContextOverflow, got {other}"),
        }
    }
}
