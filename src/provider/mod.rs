//! Provider contract — responses-style abstraction over LLM backends.
//!
//! The [`Provider`] trait defines a unified interface that council stages can
//! call instead of shelling out to CLI commands.  Each implementation wraps a
//! specific backend (Claude, Gemini, Codex, or a local model) and returns a
//! structured [`Response`] that includes token usage, enabling first-class
//! token accounting without scraping process output.
//!
//! Design notes:
//! - Modelled after the `OpenAI` Responses API shape: a request goes in, a
//!   response with `output`, `usage`, and `model` comes back.
//! - [`Tokenizer`] is a companion trait that lets callers estimate token
//!   counts *before* submission (budget pre-flight).
//! - [`TokenAccounting`] hooks observe every completion and maintain a
//!   running ledger so council runs can enforce per-run and global budgets.

pub mod accounting;
pub mod tokenizer;

use std::fmt;

use serde::{Deserialize, Serialize};

// Re-exports for convenience.
pub use accounting::{TokenAccounting, TokenBudget, TokenLedger, UsageEvent};
pub use tokenizer::{CharEstimateTokenizer, Tokenizer};

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

/// Role of a message in the conversation context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
        }
    }
}

/// A single message in the request's `input` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMessage {
    pub role: Role,
    pub content: String,
}

/// Parameters that control generation behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationParams {
    /// Maximum number of tokens the model may produce.
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    /// Sampling temperature ∈ [0.0, 2.0].
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    /// Optional stop sequences.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
}

fn default_max_output_tokens() -> u32 {
    4096
}
fn default_temperature() -> f32 {
    0.7
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self {
            max_output_tokens: default_max_output_tokens(),
            temperature: default_temperature(),
            stop: Vec::new(),
        }
    }
}

/// A completion request sent to a [`Provider`].
///
/// Mirrors the shape of the Responses API: an ordered list of input messages
/// plus generation parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Model identifier (e.g. `"claude-sonnet-4-20250514"`, `"gemini-2.5-pro"`).
    pub model: String,
    /// Ordered conversation context.
    pub input: Vec<InputMessage>,
    /// Generation parameters.
    #[serde(default)]
    pub params: GenerationParams,
    /// Opaque metadata forwarded to accounting hooks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

/// Token usage counters returned by the provider.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Tokens consumed by the input/prompt.
    pub input_tokens: u64,
    /// Tokens produced by the model.
    pub output_tokens: u64,
}

impl TokenUsage {
    #[must_use]
    pub fn total(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

/// Reason the model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of response.
    EndTurn,
    /// Hit `max_output_tokens`.
    MaxTokens,
    /// Hit a stop sequence.
    StopSequence,
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndTurn => write!(f, "end_turn"),
            Self::MaxTokens => write!(f, "max_tokens"),
            Self::StopSequence => write!(f, "stop_sequence"),
        }
    }
}

/// A structured response from a [`Provider`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Model that produced the response.
    pub model: String,
    /// The generated text output.
    pub output: String,
    /// Why the model stopped.
    pub stop_reason: StopReason,
    /// Token usage counters (input + output).
    pub usage: TokenUsage,
}

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

/// Errors returned by [`Provider::complete`].
#[derive(Debug)]
pub enum ProviderError {
    /// The request would exceed the provider's context window.
    ContextOverflow { limit: u64, requested: u64 },
    /// The pre-flight budget check failed.
    BudgetExhausted { remaining: u64, requested: u64 },
    /// Rate-limited by the upstream API.
    RateLimited { retry_after_ms: Option<u64> },
    /// Authentication failure.
    AuthError(String),
    /// Any other transport or API error.
    Other(anyhow::Error),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextOverflow { limit, requested } => {
                write!(
                    f,
                    "context overflow: {requested} tokens requested, limit is {limit}"
                )
            }
            Self::BudgetExhausted {
                remaining,
                requested,
            } => {
                write!(
                    f,
                    "budget exhausted: {requested} tokens requested, {remaining} remaining"
                )
            }
            Self::RateLimited { retry_after_ms } => match retry_after_ms {
                Some(ms) => write!(f, "rate limited, retry after {ms}ms"),
                None => write!(f, "rate limited"),
            },
            Self::AuthError(msg) => write!(f, "auth error: {msg}"),
            Self::Other(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ProviderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

impl From<anyhow::Error> for ProviderError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err)
    }
}

/// The core provider contract.
///
/// Implementations wrap a specific LLM backend (HTTP API, local binary, etc.)
/// and translate the unified [`Request`] / [`Response`] types into the
/// backend's native protocol.
///
/// # Tokenizer hook
///
/// Every provider exposes a [`Tokenizer`] via [`Provider::tokenizer`] so
/// callers can estimate input size before calling [`Provider::complete`].
///
/// # Accounting hook
///
/// After each completion, the caller is responsible for forwarding the
/// [`Response::usage`] to a [`TokenAccounting`] hook.  This keeps the
/// provider itself stateless while enabling budget enforcement at a higher
/// level (see [`TokenLedger`]).
pub trait Provider {
    /// Human-readable label (e.g. `"Claude"`, `"Gemini"`).
    fn name(&self) -> &str;

    /// The context window size in tokens for the current model configuration.
    fn context_window(&self) -> u64;

    /// Return a tokenizer suitable for this provider's model.
    fn tokenizer(&self) -> &dyn Tokenizer;

    /// Send a completion request and block until the response is available.
    ///
    /// Implementations MUST populate [`Response::usage`] with accurate token
    /// counts when the backend provides them, or best-effort estimates when
    /// it does not.
    fn complete(&self, request: &Request) -> Result<Response, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_usage_total() {
        let u = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
        };
        assert_eq!(u.total(), 150);
    }

    #[test]
    fn token_usage_total_saturates() {
        let u = TokenUsage {
            input_tokens: u64::MAX,
            output_tokens: 1,
        };
        assert_eq!(u.total(), u64::MAX);
    }

    #[test]
    fn stop_reason_display() {
        assert_eq!(StopReason::EndTurn.to_string(), "end_turn");
        assert_eq!(StopReason::MaxTokens.to_string(), "max_tokens");
        assert_eq!(StopReason::StopSequence.to_string(), "stop_sequence");
    }

    #[test]
    fn role_display() {
        assert_eq!(Role::System.to_string(), "system");
        assert_eq!(Role::User.to_string(), "user");
        assert_eq!(Role::Assistant.to_string(), "assistant");
    }

    #[test]
    fn request_serialization_roundtrip() {
        let req = Request {
            model: "test-model".to_string(),
            input: vec![
                InputMessage {
                    role: Role::System,
                    content: "You are helpful.".to_string(),
                },
                InputMessage {
                    role: Role::User,
                    content: "Hello".to_string(),
                },
            ],
            params: GenerationParams::default(),
            metadata: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let restored: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.model, "test-model");
        assert_eq!(restored.input.len(), 2);
        assert_eq!(restored.input[0].role, Role::System);
    }

    #[test]
    fn response_serialization_roundtrip() {
        let resp = Response {
            model: "test-model".to_string(),
            output: "Hello back!".to_string(),
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let restored: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.output, "Hello back!");
        assert_eq!(restored.usage.total(), 15);
        assert_eq!(restored.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn provider_error_display() {
        let overflow = ProviderError::ContextOverflow {
            limit: 100_000,
            requested: 120_000,
        };
        assert!(overflow.to_string().contains("120000"));

        let budget = ProviderError::BudgetExhausted {
            remaining: 500,
            requested: 1000,
        };
        assert!(budget.to_string().contains("500"));

        let rate = ProviderError::RateLimited {
            retry_after_ms: Some(5000),
        };
        assert!(rate.to_string().contains("5000"));

        let auth = ProviderError::AuthError("bad key".to_string());
        assert!(auth.to_string().contains("bad key"));
    }

    #[test]
    fn generation_params_defaults() {
        let params = GenerationParams::default();
        assert_eq!(params.max_output_tokens, 4096);
        assert!((params.temperature - 0.7).abs() < f32::EPSILON);
        assert!(params.stop.is_empty());
    }
}
