//! Context assembly and compaction: build messages within a token budget.

use std::sync::Arc;

use tracing::debug;

use layers_core::{
    CompactionResult, Message, MessageContent, MessageRole,
    Result, SessionStore, TokenBudget, Tokenizer,
};

// ---------------------------------------------------------------------------
// Context assembler
// ---------------------------------------------------------------------------

/// Assembles the messages array for a model call, respecting the token budget.
pub struct ContextAssembler {
    /// Optional tokenizer for accurate counting. Falls back to char-based estimate.
    tokenizer: Option<Arc<dyn Tokenizer>>,
    /// Maximum characters for a single tool result before pruning.
    max_tool_result_chars: usize,
    /// Chars-per-token estimate when no tokenizer is available.
    chars_per_token: usize,
}

impl ContextAssembler {
    pub fn new(tokenizer: Option<Arc<dyn Tokenizer>>) -> Self {
        Self {
            tokenizer,
            max_tool_result_chars: 50_000,
            chars_per_token: 4,
        }
    }

    pub fn with_max_tool_result_chars(mut self, max: usize) -> Self {
        self.max_tool_result_chars = max;
        self
    }

    /// Assemble context messages within the token budget.
    ///
    /// Strategy:
    /// 1. Count system prompt tokens.
    /// 2. Reserve output + tool budgets.
    /// 3. Add messages from most-recent backwards until budget exhausted.
    /// 4. Prune oversized tool results.
    /// 5. If still over budget, hard-trim oldest messages.
    pub async fn assemble(
        &self,
        session_id: &str,
        store: &Arc<dyn SessionStore>,
        budget: &TokenBudget,
        system_prompt: &str,
    ) -> Result<Vec<Message>> {
        // Fetch all messages for the session.
        let all_messages = store.get_messages(session_id, None).await?;

        let system_tokens = self.count_text_tokens(system_prompt);
        let tool_reserve = budget.reserved_for_tools.unwrap_or(0);
        let available = budget
            .max_input
            .saturating_sub(system_tokens)
            .saturating_sub(tool_reserve);

        // Add messages from most recent, working backwards.
        let mut selected: Vec<Message> = Vec::new();
        let mut used_tokens: usize = 0;

        for msg in all_messages.iter().rev() {
            let pruned = self.maybe_prune_tool_result(msg);
            let msg_tokens = self.count_message_tokens(&pruned);

            if used_tokens + msg_tokens > available {
                debug!(
                    session_id = session_id,
                    selected = selected.len(),
                    skipped_remaining = true,
                    "context budget reached"
                );
                break;
            }

            used_tokens += msg_tokens;
            selected.push(pruned);
        }

        // Reverse to chronological order.
        selected.reverse();

        // Ensure we don't start with a Tool message (models expect User or System first).
        while selected.first().map_or(false, |m| m.role == MessageRole::Tool) {
            selected.remove(0);
        }

        Ok(selected)
    }

    /// Prune an oversized tool result message.
    fn maybe_prune_tool_result(&self, msg: &Message) -> Message {
        if msg.role != MessageRole::Tool {
            return msg.clone();
        }

        match &msg.content {
            MessageContent::Text(text) if text.len() > self.max_tool_result_chars => {
                let truncated = format!(
                    "{}...\n[truncated: {} chars total]",
                    &text[..self.max_tool_result_chars / 2],
                    text.len()
                );
                let mut pruned = msg.clone();
                pruned.content = MessageContent::Text(truncated);
                pruned
            }
            _ => msg.clone(),
        }
    }

    fn count_text_tokens(&self, text: &str) -> usize {
        if let Some(ref tok) = self.tokenizer {
            tok.count_text_tokens(text)
        } else {
            text.len() / self.chars_per_token
        }
    }

    fn count_message_tokens(&self, msg: &Message) -> usize {
        if let Some(ref tok) = self.tokenizer {
            tok.count_message_tokens(std::slice::from_ref(msg))
        } else {
            let text_len = match &msg.content {
                MessageContent::Text(t) => t.len(),
                MessageContent::Parts(parts) => parts.iter().map(|p| {
                    match p {
                        layers_core::ContentPart::Text { text } => text.len(),
                        _ => 100, // Rough estimate for non-text parts.
                    }
                }).sum(),
            };
            // Add overhead for tool calls serialized in the message.
            let tc_len = msg
                .tool_calls
                .as_ref()
                .map(|tcs| {
                    tcs.iter()
                        .map(|tc| tc.function.name.len() + tc.function.arguments.len() + 20)
                        .sum::<usize>()
                })
                .unwrap_or(0);

            (text_len + tc_len) / self.chars_per_token
        }
    }
}

// ---------------------------------------------------------------------------
// Compaction (summarize older messages)
// ---------------------------------------------------------------------------

/// Compact older messages in a session by summarizing them.
/// This is a placeholder — real implementation would call the model for summarization.
pub async fn compact_session(
    session_id: &str,
    store: &Arc<dyn SessionStore>,
    _keep_recent: usize,
) -> Result<CompactionResult> {
    let messages = store.get_messages(session_id, None).await?;
    let original_count = messages.len();

    // Placeholder: in production, this would:
    // 1. Take older messages (beyond `keep_recent`)
    // 2. Send them to a model for summarization
    // 3. Replace them with a summary message
    // 4. Flush relevant facts to memory store
    //
    // For now, just report current state.
    Ok(CompactionResult {
        original_tokens: messages.iter().map(|m| {
            match &m.content {
                MessageContent::Text(t) => t.len() / 4,
                MessageContent::Parts(_) => 100,
            }
        }).sum(),
        compacted_tokens: 0,
        messages_removed: 0,
        messages_remaining: original_count,
    })
}
