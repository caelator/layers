//! Tokenizer trait and built-in estimators.
//!
//! Provider implementations expose a [`Tokenizer`] so callers can estimate
//! token counts *before* submitting a request.  This enables:
//!
//! - **Pre-flight budget checks**: reject prompts that would exceed the
//!   remaining budget before consuming any API quota.
//! - **Context-window fitting**: truncate or summarise context to stay
//!   within the model's window.
//! - **Cost forecasting**: display estimated cost before expensive runs.
//!
//! When a provider-specific tokenizer (e.g. tiktoken for `OpenAI`, or the
//! Claude tokenizer) is not available, [`CharEstimateTokenizer`] provides a
//! reasonable heuristic (≈ 4 characters per token for English text).

use super::{InputMessage, Request};

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Estimate token counts for text or structured requests.
///
/// Implementations may be exact (using the model's actual tokenizer) or
/// heuristic (character-ratio estimation).  The [`is_exact`](Tokenizer::is_exact)
/// method lets callers decide how much margin to add.
pub trait Tokenizer: Send + Sync {
    /// Whether this tokenizer produces exact counts for the target model.
    fn is_exact(&self) -> bool;

    /// Estimate the number of tokens in a raw text string.
    fn count_text(&self, text: &str) -> u64;

    /// Estimate the total input tokens for a full [`Request`].
    ///
    /// The default implementation sums per-message estimates plus a small
    /// overhead for message framing (role tags, separators).  Provider-
    /// specific implementations can override this for higher accuracy.
    fn count_request(&self, request: &Request) -> u64 {
        let mut total: u64 = 0;
        for msg in &request.input {
            // ~4 tokens for message framing (role, separators).
            total = total.saturating_add(4);
            total = total.saturating_add(self.count_text(&msg.content));
        }
        // +2 for the reply priming tokens.
        total.saturating_add(2)
    }

    /// Estimate tokens for a single [`InputMessage`].
    fn count_message(&self, message: &InputMessage) -> u64 {
        self.count_text(&message.content).saturating_add(4)
    }
}

// ---------------------------------------------------------------------------
// CharEstimateTokenizer
// ---------------------------------------------------------------------------

/// Character-ratio tokenizer — ≈ `chars_per_token` characters map to one token.
///
/// This is intentionally conservative (defaults to 4.0 for English) so that
/// budget pre-flight checks over-estimate rather than under-estimate.
///
/// Use this as the fallback when a model-specific tokenizer is unavailable.
#[derive(Debug, Clone)]
pub struct CharEstimateTokenizer {
    chars_per_token: f64,
}

impl CharEstimateTokenizer {
    /// Default ratio suitable for English text with the major LLM tokenizers.
    const DEFAULT_CHARS_PER_TOKEN: f64 = 4.0;

    /// Create a tokenizer with the default 4-chars-per-token ratio.
    #[must_use]
    pub fn new() -> Self {
        Self {
            chars_per_token: Self::DEFAULT_CHARS_PER_TOKEN,
        }
    }

    /// Create a tokenizer with a custom chars-per-token ratio.
    ///
    /// # Panics
    /// Panics if `ratio` is not finite or is ≤ 0.
    #[must_use]
    pub fn with_ratio(ratio: f64) -> Self {
        assert!(ratio.is_finite() && ratio > 0.0, "ratio must be > 0");
        Self {
            chars_per_token: ratio,
        }
    }
}

impl Default for CharEstimateTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Tokenizer for CharEstimateTokenizer {
    fn is_exact(&self) -> bool {
        false
    }

    fn count_text(&self, text: &str) -> u64 {
        let chars = text.len() as f64;
        // Ceiling division: ensures we never under-count.
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let tokens = (chars / self.chars_per_token).ceil() as u64;
        tokens.max(1) // even an empty string costs at least 1 token
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{GenerationParams, Role};

    fn make_tokenizer() -> CharEstimateTokenizer {
        CharEstimateTokenizer::new()
    }

    #[test]
    fn empty_text_returns_at_least_one() {
        let t = make_tokenizer();
        assert_eq!(t.count_text(""), 1);
    }

    #[test]
    fn short_text_rounds_up() {
        let t = make_tokenizer();
        // "Hi" = 2 chars → ceil(2/4) = 1
        assert_eq!(t.count_text("Hi"), 1);
        // "Hello" = 5 chars → ceil(5/4) = 2
        assert_eq!(t.count_text("Hello"), 2);
    }

    #[test]
    fn longer_text_scales_linearly() {
        let t = make_tokenizer();
        let text = "a".repeat(400);
        // 400 chars / 4.0 = 100 tokens exactly
        assert_eq!(t.count_text(&text), 100);
    }

    #[test]
    fn custom_ratio() {
        let t = CharEstimateTokenizer::with_ratio(3.0);
        // 9 chars / 3.0 = 3 tokens
        assert_eq!(t.count_text("123456789"), 3);
    }

    #[test]
    fn is_not_exact() {
        assert!(!make_tokenizer().is_exact());
    }

    #[test]
    fn count_message_includes_framing() {
        let t = make_tokenizer();
        let msg = InputMessage {
            role: Role::User,
            content: "Hello".to_string(), // 2 tokens
        };
        // 2 (text) + 4 (framing) = 6
        assert_eq!(t.count_message(&msg), 6);
    }

    #[test]
    fn count_request_sums_messages() {
        let t = make_tokenizer();
        let req = Request {
            model: "test".to_string(),
            input: vec![
                InputMessage {
                    role: Role::System,
                    content: "You are helpful.".to_string(), // ceil(16/4) = 4
                },
                InputMessage {
                    role: Role::User,
                    content: "Hi".to_string(), // ceil(2/4) = 1
                },
            ],
            params: GenerationParams::default(),
            metadata: None,
        };
        // msg0: 4 (framing) + 4 (text) = 8
        // msg1: 4 (framing) + 1 (text) = 5
        // +2 reply priming = 15
        assert_eq!(t.count_request(&req), 15);
    }

    #[test]
    #[should_panic]
    fn zero_ratio_panics() {
        let _ = CharEstimateTokenizer::with_ratio(0.0);
    }

    #[test]
    #[should_panic]
    fn negative_ratio_panics() {
        let _ = CharEstimateTokenizer::with_ratio(-1.0);
    }
}
