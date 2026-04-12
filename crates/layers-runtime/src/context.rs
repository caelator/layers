//! Context assembly and compaction: build messages within a token budget.
//!
//! t2-3: typed context assembly, compaction, multimodal pruning, and explicit
//! system prompt composition.
//!
//! Key types:
//! - [`ContextAssembler`] — selects messages within a token budget
//! - [`CompactionStrategy`] — configurable strategy for compacting old messages
//! - [`MultimodalPruner`] — strips media from messages under budget pressure
//! - [`SystemPromptComposer`] — builds the system prompt from priority sections
//! - [`ContextPlan`] — audit trail of what was included/excluded and why

use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::debug;

use layers_core::{
    CompactionResult, ContentPart, Message, MessageContent, MessageRole,
    Result, SessionStore, TokenBudget, Tokenizer,
};

// ---------------------------------------------------------------------------
// Context plan — audit trail
// ---------------------------------------------------------------------------

/// Detailed result of context assembly, recording what was included/excluded and why.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPlan {
    /// Total messages available in the session.
    pub total_available: usize,
    /// Messages selected for the context window (in chronological order).
    pub selected: Vec<SelectedMessage>,
    /// Messages excluded and the reason.
    pub excluded: Vec<ExcludedMessage>,
    /// Token accounting.
    pub token_accounting: TokenAccounting,
    /// Any pruning applied to selected messages.
    pub pruning_applied: Vec<PruneAction>,
}

/// A message selected for the context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedMessage {
    /// 0-based index in the original message list.
    pub index: usize,
    /// Role of the message.
    pub role: MessageRole,
    /// Token count (after pruning if applicable).
    pub token_count: usize,
    /// Whether this message was pruned.
    pub pruned: bool,
}

/// A message excluded from the context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludedMessage {
    /// 0-based index in the original message list.
    pub index: usize,
    /// Role of the message.
    pub role: MessageRole,
    /// Reason for exclusion.
    pub reason: ExclusionReason,
}

/// Why a message was excluded from context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// Budget exhausted before reaching this message.
    BudgetExhausted,
    /// Message was a Tool message at the start of the window (invalid for models).
    LeadingToolMessage,
    /// Message was empty after pruning.
    EmptyAfterPruning,
}

/// Token budget accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAccounting {
    /// Tokens consumed by the system prompt.
    pub system_prompt_tokens: usize,
    /// Tokens reserved for tool output.
    pub tool_reserve_tokens: usize,
    /// Tokens available for messages.
    pub available_for_messages: usize,
    /// Tokens actually used by selected messages.
    pub used_by_messages: usize,
    /// Total budget.
    pub total_budget: usize,
}

/// A pruning action applied to a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruneAction {
    /// Index of the pruned message.
    pub index: usize,
    /// Type of pruning applied.
    pub action: PruneKind,
}

/// Kind of pruning applied.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PruneKind {
    /// Oversized tool result was truncated.
    ToolResultTruncated {
        original_chars: usize,
        truncated_to: usize,
    },
    /// Media parts (images, audio, video) stripped from multimodal message.
    MediaStripped { parts_removed: usize },
}

// ---------------------------------------------------------------------------
// Compaction strategy
// ---------------------------------------------------------------------------

/// Strategy for compacting older messages when the context window is full.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStrategy {
    /// Drop oldest messages silently when budget is exhausted.
    #[default]
    DropOldest,
    /// Summarize older messages into a single system message.
    Summarize {
        /// Number of recent messages to keep unsummarized.
        keep_recent: usize,
    },
    /// Keep only the most recent N messages, discarding the rest.
    KeepOnly { count: usize },
}

// ---------------------------------------------------------------------------
// Multimodal pruner
// ---------------------------------------------------------------------------

/// Prunes media content from messages to fit within budget.
///
/// Strategy: strip images/audio/video from older messages first, keeping
/// text content. Newer messages retain their media.
pub struct MultimodalPruner {
    /// Maximum number of recent messages that retain media.
    keep_media_recent: usize,
}

impl MultimodalPruner {
    #[must_use]
    pub fn new(keep_media_recent: usize) -> Self {
        Self { keep_media_recent }
    }

    /// Strip media parts from a message if it's not in the recent window.
    ///
    /// Returns `true` if any parts were stripped.
    pub fn prune_message(&self, msg: &mut Message, distance_from_end: usize) -> bool {
        if distance_from_end < self.keep_media_recent {
            return false;
        }

        let MessageContent::Parts(parts) = &msg.content else {
            return false;
        };

        let media_count = parts
            .iter()
            .filter(|p| {
                matches!(
                    p,
                    ContentPart::ImageUrl { .. }
                        | ContentPart::AudioUrl { .. }
                        | ContentPart::VideoUrl { .. }
                )
            })
            .count();

        if media_count == 0 {
            return false;
        }

        let text_parts: Vec<ContentPart> = parts
            .iter()
            .filter(|p| matches!(p, ContentPart::Text { .. } | ContentPart::File { .. }))
            .cloned()
            .collect();

        if text_parts.is_empty() {
            msg.content = MessageContent::Text(format!(
                "[{media_count} media attachment(s) stripped for context budget]"
            ));
        } else if text_parts.len() < parts.len() {
            msg.content = MessageContent::Parts(text_parts);
        }

        debug!(
            role = ?msg.role,
            distance_from_end,
            parts_removed = media_count,
            "stripped media from older message"
        );
        true
    }
}

// ---------------------------------------------------------------------------
// System prompt composer
// ---------------------------------------------------------------------------

/// Builds the system prompt from structured components.
///
/// Instead of a single monolithic string, the system prompt is composed from
/// ordered sections with priority-based trimming.
#[derive(Debug, Clone, Default)]
pub struct SystemPromptComposer {
    sections: Vec<PromptSection>,
}

/// A single section of the system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSection {
    /// Unique label for this section (e.g. "identity", "rules", "tools").
    pub label: String,
    /// The content of this section.
    pub content: String,
    /// Priority: sections with lower priority are dropped first when trimming.
    pub priority: u8,
}

impl SystemPromptComposer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a section to the prompt.
    pub fn add_section(
        &mut self,
        label: impl Into<String>,
        content: impl Into<String>,
        priority: u8,
    ) {
        self.sections.push(PromptSection {
            label: label.into(),
            content: content.into(),
            priority,
        });
    }

    /// Compose the final system prompt string, within an optional token budget.
    ///
    /// If a tokenizer and budget are provided, low-priority sections are dropped
    /// until the prompt fits.
    pub fn compose(
        &self,
        tokenizer: Option<&Arc<dyn Tokenizer>>,
        max_tokens: Option<usize>,
    ) -> String {
        let mut sorted: Vec<&PromptSection> = self.sections.iter().collect();
        sorted.sort_by(|a, b| b.priority.cmp(&a.priority));

        let mut result = String::new();
        for section in &sorted {
            let candidate = if result.is_empty() {
                format!("## {}\n{}", section.label, section.content)
            } else {
                format!("{}\n\n## {}\n{}", result, section.label, section.content)
            };

            if let Some(max) = max_tokens {
                let estimated = match tokenizer {
                    Some(tok) => tok.count_text_tokens(&candidate),
                    None => candidate.len() / 4,
                };
                if estimated > max {
                    debug!(
                        label = %section.label,
                        priority = section.priority,
                        "dropping low-priority prompt section"
                    );
                    continue;
                }
            }

            result = candidate;
        }

        result
    }

    /// Estimate token count of the composed prompt.
    pub fn estimate_tokens(&self, tokenizer: Option<&Arc<dyn Tokenizer>>) -> usize {
        let text = self.compose(tokenizer, None);
        match tokenizer {
            Some(tok) => tok.count_text_tokens(&text),
            None => text.len() / 4,
        }
    }
}

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
    /// Compaction strategy.
    strategy: CompactionStrategy,
    /// Multimodal pruner.
    media_pruner: MultimodalPruner,
}

impl ContextAssembler {
    #[must_use]
    pub fn new(tokenizer: Option<Arc<dyn Tokenizer>>) -> Self {
        Self {
            tokenizer,
            max_tool_result_chars: 50_000,
            chars_per_token: 4,
            strategy: CompactionStrategy::default(),
            media_pruner: MultimodalPruner::new(10),
        }
    }

    #[must_use]
    pub fn with_max_tool_result_chars(mut self, max: usize) -> Self {
        self.max_tool_result_chars = max;
        self
    }

    #[must_use]
    pub fn with_strategy(mut self, strategy: CompactionStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    #[must_use]
    pub fn with_media_pruner(mut self, pruner: MultimodalPruner) -> Self {
        self.media_pruner = pruner;
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
        let all_messages = store.get_messages(session_id, None).await?;

        let system_tokens = self.count_text_tokens(system_prompt);
        let tool_reserve = budget.reserved_for_tools.unwrap_or(0);
        let available = budget
            .max_input
            .saturating_sub(system_tokens)
            .saturating_sub(tool_reserve);

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

        selected.reverse();

        while selected.first().is_some_and(|m| m.role == MessageRole::Tool) {
            selected.remove(0);
        }

        Ok(selected)
    }

    /// Produce a detailed [`ContextPlan`] showing what was included/excluded.
    pub async fn plan(
        &self,
        session_id: &str,
        store: &Arc<dyn SessionStore>,
        budget: &TokenBudget,
        system_prompt: &str,
    ) -> Result<ContextPlan> {
        let all_messages = store.get_messages(session_id, None).await?;
        let total_available = all_messages.len();

        let system_tokens = self.count_text_tokens(system_prompt);
        let tool_reserve = budget.reserved_for_tools.unwrap_or(0);
        let available = budget
            .max_input
            .saturating_sub(system_tokens)
            .saturating_sub(tool_reserve);

        let effective_messages: Vec<(usize, &Message)> = match &self.strategy {
            CompactionStrategy::KeepOnly { count } => {
                let skip = total_available.saturating_sub(*count);
                all_messages.iter().enumerate().skip(skip).collect()
            }
            _ => all_messages.iter().enumerate().collect(),
        };

        let mut selected: Vec<SelectedMessage> = Vec::new();
        let mut excluded: Vec<ExcludedMessage> = Vec::new();
        let mut pruning_applied = Vec::new();
        let mut used_tokens: usize = 0;

        let mut budget_exhausted = false;
        for (original_idx, msg) in effective_messages.iter().rev() {
            if budget_exhausted {
                excluded.push(ExcludedMessage {
                    index: *original_idx,
                    role: msg.role.clone(),
                    reason: ExclusionReason::BudgetExhausted,
                });
                continue;
            }

            let distance_from_end = effective_messages.len() - 1 - selected.len();
            let mut pruned_msg = (*msg).clone();
            let was_media_pruned = self
                .media_pruner
                .prune_message(&mut pruned_msg, distance_from_end);
            let pruned_msg = self.maybe_prune_tool_result(&pruned_msg);

            let msg_tokens = self.count_message_tokens(&pruned_msg);

            if msg_tokens == 0 {
                excluded.push(ExcludedMessage {
                    index: *original_idx,
                    role: msg.role.clone(),
                    reason: ExclusionReason::EmptyAfterPruning,
                });
                continue;
            }

            if used_tokens + msg_tokens > available {
                budget_exhausted = true;
                excluded.push(ExcludedMessage {
                    index: *original_idx,
                    role: msg.role.clone(),
                    reason: ExclusionReason::BudgetExhausted,
                });
                continue;
            }

            if was_media_pruned {
                if let MessageContent::Parts(parts) = &msg.content {
                    let media_count = parts
                        .iter()
                        .filter(|p| {
                            matches!(
                                p,
                                ContentPart::ImageUrl { .. }
                                    | ContentPart::AudioUrl { .. }
                                    | ContentPart::VideoUrl { .. }
                            )
                        })
                        .count();
                    if media_count > 0 {
                        pruning_applied.push(PruneAction {
                            index: *original_idx,
                            action: PruneKind::MediaStripped {
                                parts_removed: media_count,
                            },
                        });
                    }
                }
            }

            used_tokens += msg_tokens;
            selected.push(SelectedMessage {
                index: *original_idx,
                role: msg.role.clone(),
                token_count: msg_tokens,
                pruned: was_media_pruned,
            });
        }

        selected.reverse();

        while selected.first().is_some_and(|m| m.role == MessageRole::Tool) {
            let removed = selected.remove(0);
            excluded.push(ExcludedMessage {
                index: removed.index,
                role: removed.role,
                reason: ExclusionReason::LeadingToolMessage,
            });
        }

        Ok(ContextPlan {
            total_available,
            selected,
            excluded,
            token_accounting: TokenAccounting {
                system_prompt_tokens: system_tokens,
                tool_reserve_tokens: tool_reserve,
                available_for_messages: available,
                used_by_messages: used_tokens,
                total_budget: budget.max_input,
            },
            pruning_applied,
        })
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
            let text_len: usize = match &msg.content {
                MessageContent::Text(t) => t.len(),
                MessageContent::Parts(parts) => parts
                    .iter()
                    .map(|p| match p {
                        ContentPart::Text { text } => text.len(),
                        _ => 100,
                    })
                    .sum(),
            };
            let tc_len: usize = msg
                .tool_calls
                .as_ref()
                .map(|tcs| {
                    tcs.iter()
                        .map(|tc| tc.function.name.len() + tc.function.arguments.len() + 20)
                        .sum()
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
/// Placeholder — real implementation would call the model for summarization.
pub async fn compact_session(
    session_id: &str,
    store: &Arc<dyn SessionStore>,
    keep_recent: usize,
) -> Result<CompactionResult> {
    let messages = store.get_messages(session_id, None).await?;
    let original_count = messages.len();

    if original_count <= keep_recent {
        return Ok(CompactionResult {
            original_tokens: estimate_messages_tokens(&messages),
            compacted_tokens: 0,
            messages_removed: 0,
            messages_remaining: original_count,
        });
    }

    let older_count = original_count - keep_recent;
    let older_tokens = estimate_messages_tokens(&messages[..older_count]);
    let recent_tokens = estimate_messages_tokens(&messages[older_count..]);

    Ok(CompactionResult {
        original_tokens: older_tokens + recent_tokens,
        compacted_tokens: recent_tokens,
        messages_removed: older_count,
        messages_remaining: keep_recent,
    })
}

/// Produce a compaction summary message for the older portion of a session.
pub fn build_compaction_summary(older_messages: &[Message]) -> Message {
    let user_msgs = older_messages
        .iter()
        .filter(|m| m.role == MessageRole::User)
        .count();
    let assistant_msgs = older_messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant)
        .count();
    let tool_msgs = older_messages
        .iter()
        .filter(|m| m.role == MessageRole::Tool)
        .count();

    let summary = format!(
        "[Compaction summary of {} messages: {} user, {} assistant, {} tool calls.]",
        older_messages.len(),
        user_msgs,
        assistant_msgs,
        tool_msgs,
    );

    Message {
        role: MessageRole::System,
        content: MessageContent::Text(summary),
        name: Some("compaction".to_string()),
        tool_calls: None,
        tool_call_id: None,
        reasoning: None,
        timestamp: Some(Utc::now()),
    }
}

fn estimate_messages_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.len() / 4,
            MessageContent::Parts(parts) => {
                parts.iter().map(|p| match p {
                    ContentPart::Text { text } => text.len() / 4,
                    _ => 25,
                }).sum()
            }
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

    fn multimodal_msg(role: MessageRole, text: &str, image_url: &str) -> Message {
        Message {
            role,
            content: MessageContent::Parts(vec![
                ContentPart::Text {
                    text: text.to_string(),
                },
                ContentPart::ImageUrl {
                    image_url: layers_core::types::ImageUrl {
                        url: image_url.to_string(),
                        detail: None,
                    },
                },
            ]),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        }
    }

    #[test]
    fn multimodal_pruner_keeps_recent_media() {
        let pruner = MultimodalPruner::new(3);
        let mut msg = multimodal_msg(MessageRole::User, "see this", "https://img.png");
        assert!(!pruner.prune_message(&mut msg, 2));
        assert!(matches!(msg.content, MessageContent::Parts(_)));
    }

    #[test]
    fn multimodal_pruner_strips_old_media() {
        let pruner = MultimodalPruner::new(3);
        let mut msg = multimodal_msg(MessageRole::User, "see this", "https://img.png");
        assert!(pruner.prune_message(&mut msg, 5));
        match &msg.content {
            MessageContent::Parts(parts) => {
                assert!(parts.iter().all(|p| matches!(p, ContentPart::Text { .. })));
            }
            MessageContent::Text(t) => {
                assert!(t.contains("media attachment"));
            }
        }
    }

    #[test]
    fn multimodal_pruner_text_only_intact() {
        let pruner = MultimodalPruner::new(0);
        let mut msg = text_msg(MessageRole::User, "hello");
        assert!(!pruner.prune_message(&mut msg, 100));
    }

    #[test]
    fn composer_builds_ordered_prompt() {
        let mut composer = SystemPromptComposer::new();
        composer.add_section("identity", "You are a helpful assistant.", 10);
        composer.add_section("rules", "Always be concise.", 5);
        let prompt = composer.compose(None, None);
        let identity_pos = prompt.find("identity").expect("identity present");
        let rules_pos = prompt.find("rules").expect("rules present");
        assert!(identity_pos < rules_pos);
    }

    #[test]
    fn composer_trims_low_priority() {
        let mut composer = SystemPromptComposer::new();
        composer.add_section("identity", "short", 10);
        composer.add_section("padding", &"x".repeat(500), 1);
        let prompt = composer.compose(None, Some(50));
        assert!(prompt.contains("identity"));
        assert!(!prompt.contains("padding"));
    }

    #[test]
    fn compaction_strategy_roundtrip() {
        let strategies = vec![
            CompactionStrategy::DropOldest,
            CompactionStrategy::Summarize { keep_recent: 10 },
            CompactionStrategy::KeepOnly { count: 5 },
        ];
        for s in &strategies {
            let json = serde_json::to_string(s).unwrap();
            let back: CompactionStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, back);
        }
    }

    #[test]
    fn context_plan_serializes() {
        let plan = ContextPlan {
            total_available: 5,
            selected: vec![SelectedMessage {
                index: 0,
                role: MessageRole::User,
                token_count: 10,
                pruned: false,
            }],
            excluded: vec![ExcludedMessage {
                index: 4,
                role: MessageRole::Assistant,
                reason: ExclusionReason::BudgetExhausted,
            }],
            token_accounting: TokenAccounting {
                system_prompt_tokens: 50,
                tool_reserve_tokens: 0,
                available_for_messages: 950,
                used_by_messages: 10,
                total_budget: 1000,
            },
            pruning_applied: vec![],
        };
        let json = serde_json::to_string(&plan).unwrap();
        let back: ContextPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(plan.total_available, back.total_available);
    }

    #[test]
    fn compaction_summary_counts_roles() {
        let messages = vec![
            text_msg(MessageRole::User, "hi"),
            text_msg(MessageRole::Assistant, "hello"),
            text_msg(MessageRole::User, "how are you"),
            text_msg(MessageRole::Tool, "result"),
        ];
        let summary = build_compaction_summary(&messages);
        assert_eq!(summary.role, MessageRole::System);
        if let MessageContent::Text(t) = &summary.content {
            assert!(t.contains("2 user"));
            assert!(t.contains("1 assistant"));
            assert!(t.contains("1 tool"));
        }
    }

    #[test]
    fn estimate_tokens_works() {
        let messages = vec![
            text_msg(MessageRole::User, "hello world"),
            text_msg(MessageRole::Assistant, "hi there!"),
        ];
        assert_eq!(estimate_messages_tokens(&messages), 4);
    }
}
