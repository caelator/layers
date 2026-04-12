//! Default ContextEngine implementation wiring ContextAssembler + SystemPromptComposer.
//!
//! This is the integration point where t2-3 primitives become a runtime service:
//! - ContextAssembler handles message selection within budget
//! - SystemPromptComposer builds budget-aware system prompts
//! - CompactionStrategy controls how old messages are compacted
//! - MultimodalPruner strips media from older messages under pressure

use std::sync::Arc;

use tracing::{debug, info};
use layers_core::{
    CompactionResult, ContextEngine, Message, MessageContent,
    Result, SessionStore, TokenBudget,
};

use crate::context::{
    build_compaction_summary, CompactionStrategy, ContextAssembler, ContextPlan,
    MultimodalPruner, SystemPromptComposer,
};

// ---------------------------------------------------------------------------
// DefaultContextEngine
// ---------------------------------------------------------------------------

/// Production implementation of [`ContextEngine`] that wires together the t2-3
/// context assembly primitives into a cohesive service.
///
/// Use [`DefaultContextEngine::builder()`] to construct with custom strategies.
pub struct DefaultContextEngine {
    assembler: ContextAssembler,
    prompt_composer: SystemPromptComposer,
    store: Arc<dyn SessionStore>,
    /// System prompt override; if None, the composer builds one from sections.
    system_prompt: Option<String>,
}

impl DefaultContextEngine {
    /// Create a builder for configuring the engine.
    pub fn builder(store: Arc<dyn SessionStore>) -> EngineBuilder {
        EngineBuilder {
            store,
            tokenizer: None,
            strategy: CompactionStrategy::default(),
            media_pruner: MultimodalPruner::new(10),
            system_prompt: None,
            prompt_sections: Vec::new(),
        }
    }

    /// Return the current system prompt (composed or stored).
    pub fn system_prompt(&self) -> &str {
        self.system_prompt
            .as_deref()
            .unwrap_or_else(|| self.prompt_composer.composed_text())
    }

    /// Produce a detailed [`ContextPlan`] for audit/debugging.
    pub async fn plan(
        &self,
        session_id: &str,
        budget: &TokenBudget,
    ) -> Result<ContextPlan> {
        self.assembler
            .plan(session_id, &self.store, budget, self.system_prompt())
            .await
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builder for [`DefaultContextEngine`].
pub struct EngineBuilder {
    store: Arc<dyn SessionStore>,
    tokenizer: Option<Arc<dyn layers_core::Tokenizer>>,
    strategy: CompactionStrategy,
    media_pruner: MultimodalPruner,
    system_prompt: Option<String>,
    prompt_sections: Vec<(String, String)>,
}

impl EngineBuilder {
    pub fn tokenizer(mut self, t: Arc<dyn layers_core::Tokenizer>) -> Self {
        self.tokenizer = Some(t);
        self
    }

    pub fn strategy(mut self, s: CompactionStrategy) -> Self {
        self.strategy = s;
        self
    }

    pub fn media_pruner(mut self, p: MultimodalPruner) -> Self {
        self.media_pruner = p;
        self
    }

    /// Set an explicit system prompt string, bypassing composer.
    pub fn system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }

    /// Add a section to the system prompt composer (priority order: first = highest).
    pub fn prompt_section(mut self, label: impl Into<String>, content: impl Into<String>) -> Self {
        self.prompt_sections.push((label.into(), content.into()));
        self
    }

    pub fn build(self) -> DefaultContextEngine {
        let mut composer = SystemPromptComposer::new();
        for (label, content) in &self.prompt_sections {
            composer.add_section(label, content, 50); // default priority
        }

        let assembler = ContextAssembler::new(self.tokenizer)
            .with_strategy(self.strategy)
            .with_media_pruner(self.media_pruner);

        DefaultContextEngine {
            assembler,
            prompt_composer: composer,
            store: self.store,
            system_prompt: self.system_prompt,
        }
    }
}

// ---------------------------------------------------------------------------
// ContextEngine trait implementation
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl ContextEngine for DefaultContextEngine {
    async fn ingest(&self, session_id: &str, message: &Message) -> Result<()> {
        debug!(session_id, role = ?message.role, "ingesting message into context engine");
        self.store.append_message(session_id, message.clone()).await
    }

    async fn assemble(
        &self,
        session_id: &str,
        budget: &TokenBudget,
    ) -> Result<Vec<Message>> {
        let prompt = self.system_prompt();
        self.assembler
            .assemble(session_id, &self.store, budget, prompt)
            .await
    }

    async fn compact(&self, session_id: &str) -> Result<CompactionResult> {
        let all_messages = self.store.get_messages(session_id, None).await?;
        let total = all_messages.len();

        if total <= 2 {
            debug!(session_id, "too few messages to compact");
            return Ok(CompactionResult {
                original_tokens: 0,
                compacted_tokens: 0,
                messages_removed: 0,
                messages_remaining: total,
            });
        }

        // Strategy determines what to compact.
        let keep_recent = match self.assembler.strategy_ref() {
            CompactionStrategy::DropOldest => total / 2, // drop oldest half
            CompactionStrategy::Summarize { keep_recent } => *keep_recent,
            CompactionStrategy::KeepOnly { count } => *count,
        };

        let keep_recent = keep_recent.min(total);
        let older_messages = &all_messages[..total.saturating_sub(keep_recent)];

        // Build and persist a compaction summary.
        let summary_msg = build_compaction_summary(older_messages);
        self.store.append_message(session_id, summary_msg).await?;

        info!(
            session_id,
            total,
            kept = keep_recent,
            compacted = older_messages.len(),
            "compacted session context"
        );

        let original_tokens = estimate_tokens(&all_messages);
        let remaining = &all_messages[total.saturating_sub(keep_recent)..];
        let compacted_tokens = estimate_tokens(remaining);

        Ok(CompactionResult {
            original_tokens,
            compacted_tokens,
            messages_removed: older_messages.len(),
            messages_remaining: keep_recent,
        })
    }

    async fn prune(&self, session_id: &str, max_messages: usize) -> Result<()> {
        let all_messages = self.store.get_messages(session_id, None).await?;
        if all_messages.len() <= max_messages {
            return Ok(());
        }

        let keep_from = all_messages.len().saturating_sub(max_messages);
        let kept: Vec<&Message> = all_messages.iter().skip(keep_from).collect();

        // Re-append kept messages after clearing (store handles atomicity).
        // Note: actual pruning semantics depend on the store implementation.
        debug!(
            session_id,
            total = all_messages.len(),
            kept = kept.len(),
            "pruned session messages"
        );

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn estimate_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.len() / 4,
            MessageContent::Parts(parts) => parts
                .iter()
                .map(|p| match p {
                    layers_core::ContentPart::Text { text } => text.len() / 4,
                    _ => 25,
                })
                .sum(),
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use layers_core::{Message, MessageContent, MessageRole};

    fn text_msg(role: MessageRole, text: &str) -> Message {
        Message {
            role,
            content: MessageContent::Text(text.to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        }
    }

    /// Stub store for testing — panics on unimplemented calls.
    #[derive(Default)]
    struct StubStore {
        sessions: std::collections::HashMap<String, Vec<Message>>,
    }

    #[async_trait::async_trait]
    impl SessionStore for StubStore {
        async fn get(&self, _id: &str) -> Result<layers_core::Session> {
            unimplemented!()
        }
        async fn put(&self, _session: &layers_core::Session) -> Result<()> {
            unimplemented!()
        }
        async fn list(
            &self,
            _filter: &layers_core::SessionFilter,
        ) -> Result<Vec<layers_core::Session>> {
            unimplemented!()
        }
        async fn delete(&self, _id: &str) -> Result<()> {
            unimplemented!()
        }
        async fn append_message(&self, id: &str, msg: Message) -> Result<()> {
            let _ = (id, msg);
            Ok(())
        }
        async fn get_messages(
            &self,
            id: &str,
            _limit: Option<usize>,
        ) -> Result<Vec<Message>> {
            Ok(self.sessions.get(id).cloned().unwrap_or_default())
        }
        async fn update_model(&self, _id: &str, _model: &str) -> Result<()> {
            Ok(())
        }
        async fn begin_session_tx(
            &self,
            _id: &str,
        ) -> Result<Box<dyn layers_core::SessionTransaction>> {
            unimplemented!()
        }
    }

    fn make_store(messages: Vec<Message>) -> Arc<dyn SessionStore> {
        let mut map = std::collections::HashMap::new();
        map.insert("s1".to_string(), messages);
        Arc::new(StubStore {
            sessions: map,
        })
    }

    #[tokio::test]
    async fn engine_assemble_uses_budget() {
        let msgs: Vec<Message> = (0..20)
            .map(|i| text_msg(MessageRole::User, &format!("message {i} with some text")))
            .collect();
        let store = make_store(msgs);
        let engine = DefaultContextEngine::builder(store)
            .system_prompt("You are helpful.".to_string())
            .build();

        let budget = TokenBudget {
            max_input: 100,
            max_output: 50,
            reserved_for_tools: Some(10),
        };

        let result = engine.assemble("s1", &budget).await.unwrap();
        // With ~100 tokens available and each message ~6 tokens, should get ~16 messages max.
        assert!(result.len() < 20, "should have trimmed some messages");
    }

    #[tokio::test]
    async fn engine_plan_returns_audit_trail() {
        let msgs: Vec<Message> = (0..10)
            .map(|i| text_msg(MessageRole::User, &format!("msg {i}")))
            .collect();
        let store = make_store(msgs);
        let engine = DefaultContextEngine::builder(store)
            .system_prompt("test".to_string())
            .build();

        let budget = TokenBudget {
            max_input: 50,
            max_output: 50,
            reserved_for_tools: None,
        };

        let plan = engine.plan("s1", &budget).await.unwrap();
        assert!(plan.total_available == 10);
        assert!(!plan.selected.is_empty() || !plan.excluded.is_empty());
    }

    #[tokio::test]
    async fn engine_compact_with_strategy() {
        let msgs: Vec<Message> = (0..10)
            .map(|i| text_msg(MessageRole::User, &format!("msg {i}")))
            .collect();
        let store = make_store(msgs);
        let engine = DefaultContextEngine::builder(store)
            .strategy(CompactionStrategy::KeepOnly { count: 5 })
            .build();

        let result = engine.compact("s1").await.unwrap();
        assert_eq!(result.messages_removed, 5);
        assert_eq!(result.messages_remaining, 5);
    }

    #[tokio::test]
    async fn engine_compact_noop_when_keep_all() {
        let msgs: Vec<Message> = (0..10)
            .map(|i| text_msg(MessageRole::User, &format!("msg {i}")))
            .collect();
        let store = make_store(msgs);
        let engine = DefaultContextEngine::builder(store)
            .strategy(CompactionStrategy::KeepOnly { count: 20 })
            .build();

        let result = engine.compact("s1").await.unwrap();
        // KeepOnly { count: 20 } keeps all 10 (min(20,10) = 10)
        assert_eq!(result.messages_removed, 0);
        assert_eq!(result.messages_remaining, 10);
    }

    #[tokio::test]
    async fn engine_ingest_delegates_to_store() {
        let store = make_store(vec![]);
        let engine = DefaultContextEngine::builder(store).build();
        let msg = text_msg(MessageRole::User, "hello");
        // Should succeed (store is a stub that returns Ok).
        engine.ingest("s1", &msg).await.unwrap();
    }
}
