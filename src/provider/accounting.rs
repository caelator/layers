//! Token accounting — budget enforcement and usage ledger.
//!
//! After each [`Provider::complete`](super::Provider::complete) call the
//! caller forwards the [`TokenUsage`](super::TokenUsage) to a
//! [`TokenAccounting`] hook.  The hook can enforce budgets, log usage, or
//! feed metrics to the telemetry plugin — without coupling the provider
//! itself to any particular accounting policy.
//!
//! [`TokenLedger`] is the default concrete implementation: it tracks
//! cumulative usage per model, enforces an optional per-run budget, and
//! produces a serializable summary suitable for appending to council
//! trace records.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::TokenUsage;

// ---------------------------------------------------------------------------
// UsageEvent — the unit of accounting
// ---------------------------------------------------------------------------

/// A single token-usage observation forwarded from a provider completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    /// Model that produced the completion.
    pub model: String,
    /// Council stage that requested the completion (e.g. `"gemini"`, `"claude"`).
    pub stage: String,
    /// Token counts.
    pub usage: TokenUsage,
    /// ISO-8601 timestamp of the completion.
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// TokenBudget
// ---------------------------------------------------------------------------

/// An optional ceiling on total token consumption for a run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Maximum total (input + output) tokens allowed.
    pub max_total_tokens: u64,
}

impl TokenBudget {
    #[must_use]
    pub fn new(max_total_tokens: u64) -> Self {
        Self { max_total_tokens }
    }

    /// How many tokens remain given `consumed` so far.
    #[must_use]
    pub fn remaining(&self, consumed: u64) -> u64 {
        self.max_total_tokens.saturating_sub(consumed)
    }

    /// Whether the budget would be exceeded by an additional `requested`.
    #[must_use]
    pub fn would_exceed(&self, consumed: u64, requested: u64) -> bool {
        consumed.saturating_add(requested) > self.max_total_tokens
    }
}

// ---------------------------------------------------------------------------
// TokenAccounting trait
// ---------------------------------------------------------------------------

/// Hook invoked after every provider completion.
///
/// Implementations decide what to *do* with the usage data: enforce budgets,
/// write to a ledger file, send to telemetry, etc.
pub trait TokenAccounting: Send + Sync {
    /// Record a completed usage event.
    ///
    /// Returns `Err` if the budget is exhausted and the caller should stop
    /// issuing further completions.
    fn record(&mut self, event: &UsageEvent) -> Result<(), BudgetExceeded>;

    /// Pre-flight check: will `additional` tokens fit within the budget?
    ///
    /// Returns `Ok(remaining)` or `Err(BudgetExceeded)`.
    fn check_budget(&self, additional: u64) -> Result<u64, BudgetExceeded>;

    /// Cumulative total tokens consumed so far.
    fn total_consumed(&self) -> u64;

    /// Per-model breakdown of consumed tokens.
    fn per_model_usage(&self) -> HashMap<String, TokenUsage>;
}

/// Returned when a budget ceiling is hit.
#[derive(Debug, Clone)]
pub struct BudgetExceeded {
    pub limit: u64,
    pub consumed: u64,
    pub requested: u64,
}

impl std::fmt::Display for BudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "token budget exceeded: limit={}, consumed={}, requested={}",
            self.limit, self.consumed, self.requested
        )
    }
}

impl std::error::Error for BudgetExceeded {}

// ---------------------------------------------------------------------------
// TokenLedger — default implementation
// ---------------------------------------------------------------------------

/// Per-model accumulator.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ModelAccumulator {
    input_tokens: u64,
    output_tokens: u64,
    completions: u64,
}

/// Concrete [`TokenAccounting`] implementation that maintains an in-memory
/// ledger of all usage events and enforces an optional budget.
///
/// Designed to live for the duration of a single council run.  The full
/// event log and summary are serializable for persistence to artifact files.
#[derive(Debug, Clone)]
pub struct TokenLedger {
    budget: Option<TokenBudget>,
    total_input: u64,
    total_output: u64,
    per_model: HashMap<String, ModelAccumulator>,
    events: Vec<UsageEvent>,
}

impl TokenLedger {
    /// Create a ledger with no budget limit.
    #[must_use]
    pub fn new() -> Self {
        Self {
            budget: None,
            total_input: 0,
            total_output: 0,
            per_model: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// Create a ledger that enforces `budget`.
    #[must_use]
    pub fn with_budget(budget: TokenBudget) -> Self {
        Self {
            budget: Some(budget),
            ..Self::new()
        }
    }

    /// Total input tokens consumed.
    #[must_use]
    pub fn total_input_tokens(&self) -> u64 {
        self.total_input
    }

    /// Total output tokens consumed.
    #[must_use]
    pub fn total_output_tokens(&self) -> u64 {
        self.total_output
    }

    /// Number of completion calls recorded.
    #[must_use]
    pub fn completion_count(&self) -> usize {
        self.events.len()
    }

    /// The full ordered event log.
    #[must_use]
    pub fn events(&self) -> &[UsageEvent] {
        &self.events
    }

    /// Produce a serializable summary suitable for appending to trace records.
    #[must_use]
    pub fn summary(&self) -> LedgerSummary {
        LedgerSummary {
            total_input_tokens: self.total_input,
            total_output_tokens: self.total_output,
            total_tokens: self.total_consumed(),
            completions: self.events.len() as u64,
            budget_limit: self.budget.map(|b| b.max_total_tokens),
            per_model: self
                .per_model
                .iter()
                .map(|(model, acc)| {
                    (
                        model.clone(),
                        ModelUsageSummary {
                            input_tokens: acc.input_tokens,
                            output_tokens: acc.output_tokens,
                            completions: acc.completions,
                        },
                    )
                })
                .collect(),
        }
    }
}

impl Default for TokenLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenAccounting for TokenLedger {
    fn record(&mut self, event: &UsageEvent) -> Result<(), BudgetExceeded> {
        let additional = event.usage.total();

        if let Some(budget) = &self.budget {
            if budget.would_exceed(self.total_consumed(), additional) {
                return Err(BudgetExceeded {
                    limit: budget.max_total_tokens,
                    consumed: self.total_consumed(),
                    requested: additional,
                });
            }
        }

        self.total_input = self.total_input.saturating_add(event.usage.input_tokens);
        self.total_output = self.total_output.saturating_add(event.usage.output_tokens);

        let acc = self
            .per_model
            .entry(event.model.clone())
            .or_default();
        acc.input_tokens = acc.input_tokens.saturating_add(event.usage.input_tokens);
        acc.output_tokens = acc.output_tokens.saturating_add(event.usage.output_tokens);
        acc.completions = acc.completions.saturating_add(1);

        self.events.push(event.clone());
        Ok(())
    }

    fn check_budget(&self, additional: u64) -> Result<u64, BudgetExceeded> {
        match &self.budget {
            Some(budget) => {
                if budget.would_exceed(self.total_consumed(), additional) {
                    Err(BudgetExceeded {
                        limit: budget.max_total_tokens,
                        consumed: self.total_consumed(),
                        requested: additional,
                    })
                } else {
                    Ok(budget.remaining(self.total_consumed()))
                }
            }
            None => Ok(u64::MAX),
        }
    }

    fn total_consumed(&self) -> u64 {
        self.total_input.saturating_add(self.total_output)
    }

    fn per_model_usage(&self) -> HashMap<String, TokenUsage> {
        self.per_model
            .iter()
            .map(|(model, acc)| {
                (
                    model.clone(),
                    TokenUsage {
                        input_tokens: acc.input_tokens,
                        output_tokens: acc.output_tokens,
                    },
                )
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Serializable summary
// ---------------------------------------------------------------------------

/// Per-model usage in the summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub completions: u64,
}

/// Serializable snapshot of the ledger state, suitable for JSON persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerSummary {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub completions: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_limit: Option<u64>,
    pub per_model: HashMap<String, ModelUsageSummary>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(model: &str, stage: &str, input: u64, output: u64) -> UsageEvent {
        UsageEvent {
            model: model.to_string(),
            stage: stage.to_string(),
            usage: TokenUsage {
                input_tokens: input,
                output_tokens: output,
            },
            timestamp: "2026-04-11T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn ledger_tracks_cumulative_usage() {
        let mut ledger = TokenLedger::new();
        ledger
            .record(&make_event("claude", "claude", 100, 50))
            .unwrap();
        ledger
            .record(&make_event("gemini", "gemini", 200, 80))
            .unwrap();

        assert_eq!(ledger.total_input_tokens(), 300);
        assert_eq!(ledger.total_output_tokens(), 130);
        assert_eq!(ledger.total_consumed(), 430);
        assert_eq!(ledger.completion_count(), 2);
    }

    #[test]
    fn ledger_per_model_breakdown() {
        let mut ledger = TokenLedger::new();
        ledger
            .record(&make_event("claude", "claude", 100, 50))
            .unwrap();
        ledger
            .record(&make_event("claude", "claude", 200, 80))
            .unwrap();
        ledger
            .record(&make_event("gemini", "gemini", 300, 100))
            .unwrap();

        let by_model = ledger.per_model_usage();
        assert_eq!(by_model.len(), 2);
        let claude = by_model.get("claude").unwrap();
        assert_eq!(claude.input_tokens, 300);
        assert_eq!(claude.output_tokens, 130);
        let gemini = by_model.get("gemini").unwrap();
        assert_eq!(gemini.input_tokens, 300);
        assert_eq!(gemini.output_tokens, 100);
    }

    #[test]
    fn budget_enforcement_blocks_over_limit() {
        let budget = TokenBudget::new(500);
        let mut ledger = TokenLedger::with_budget(budget);

        // First event: 300 tokens — fits.
        ledger
            .record(&make_event("claude", "claude", 200, 100))
            .unwrap();
        assert_eq!(ledger.total_consumed(), 300);

        // Pre-flight check: 200 more would hit exactly 500 — fits.
        assert!(ledger.check_budget(200).is_ok());

        // Pre-flight check: 201 would exceed — blocked.
        assert!(ledger.check_budget(201).is_err());

        // Record that pushes past budget — blocked.
        let result = ledger.record(&make_event("gemini", "gemini", 150, 100));
        assert!(result.is_err());
        // Ledger should not have recorded the rejected event.
        assert_eq!(ledger.total_consumed(), 300);
        assert_eq!(ledger.completion_count(), 1);
    }

    #[test]
    fn no_budget_always_passes_check() {
        let ledger = TokenLedger::new();
        assert_eq!(ledger.check_budget(u64::MAX).unwrap(), u64::MAX);
    }

    #[test]
    fn budget_remaining() {
        let budget = TokenBudget::new(1000);
        assert_eq!(budget.remaining(400), 600);
        assert_eq!(budget.remaining(1000), 0);
        assert_eq!(budget.remaining(1200), 0); // saturates
    }

    #[test]
    fn summary_serialization() {
        let mut ledger = TokenLedger::with_budget(TokenBudget::new(10_000));
        ledger
            .record(&make_event("claude", "claude", 500, 200))
            .unwrap();
        ledger
            .record(&make_event("gemini", "gemini", 800, 300))
            .unwrap();

        let summary = ledger.summary();
        let json = serde_json::to_string_pretty(&summary).unwrap();
        let restored: LedgerSummary = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.total_tokens, 1800);
        assert_eq!(restored.completions, 2);
        assert_eq!(restored.budget_limit, Some(10_000));
        assert_eq!(restored.per_model.len(), 2);
    }

    #[test]
    fn budget_exceeded_display() {
        let err = BudgetExceeded {
            limit: 1000,
            consumed: 800,
            requested: 300,
        };
        let msg = err.to_string();
        assert!(msg.contains("1000"));
        assert!(msg.contains("800"));
        assert!(msg.contains("300"));
    }

    #[test]
    fn events_are_ordered() {
        let mut ledger = TokenLedger::new();
        ledger
            .record(&make_event("gemini", "gemini", 100, 50))
            .unwrap();
        ledger
            .record(&make_event("claude", "claude", 200, 80))
            .unwrap();
        ledger
            .record(&make_event("codex", "codex", 300, 100))
            .unwrap();

        let events = ledger.events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].stage, "gemini");
        assert_eq!(events[1].stage, "claude");
        assert_eq!(events[2].stage, "codex");
    }
}
