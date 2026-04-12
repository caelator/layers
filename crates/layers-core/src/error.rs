use std::time::Duration;

/// Unified error type for the Layers framework.
#[derive(Debug, thiserror::Error)]
pub enum LayersError {
    #[error("provider error: {0}")]
    Provider(String),

    #[error("rate limited: retry after {retry_after:?}")]
    RateLimited { retry_after: Option<Duration> },

    #[error("context overflow: {used} tokens used, {limit} limit")]
    ContextOverflow { used: usize, limit: usize },

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("tool error: {0}")]
    Tool(String),

    #[error("channel error: {0}")]
    Channel(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("all fallback providers exhausted")]
    FallbackExhausted,

    #[error("operation timed out after {0:?}")]
    Timeout(Duration),

    #[error("operation cancelled")]
    Cancelled,
}

/// Convenience result type for Layers operations.
pub type Result<T> = std::result::Result<T, LayersError>;
