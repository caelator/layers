//! Tool loop detection: detect repeating tool call patterns.

use std::collections::HashMap;

/// Detects when the same tool+args combination is called repeatedly.
pub struct LoopDetector {
    /// Map of (tool_name, args_hash) → call count.
    calls: HashMap<(String, String), usize>,
    /// Total tool calls in this detection window.
    total_calls: usize,
    /// Maximum iterations before declaring a global loop.
    max_iterations: usize,
    /// Number of times the same (tool, args) can repeat before it's a loop.
    repeat_threshold: usize,
}

impl LoopDetector {
    /// Create a new loop detector with default limits.
    #[must_use]
    pub fn new() -> Self {
        Self {
            calls: HashMap::new(),
            total_calls: 0,
            max_iterations: 50,
            repeat_threshold: 3,
        }
    }

    /// Set the maximum total iteration count.
    #[must_use]
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    /// Set the repeat threshold for same (tool, args) detection.
    #[must_use]
    pub fn with_repeat_threshold(mut self, threshold: usize) -> Self {
        self.repeat_threshold = threshold;
        self
    }

    /// Record a tool call and check for loops.
    ///
    /// Returns `Some(reason)` if a loop is detected, `None` otherwise.
    pub fn record(&mut self, tool_name: &str, args: &serde_json::Value) -> Option<LoopReason> {
        self.total_calls += 1;

        // Check global iteration limit.
        if self.total_calls >= self.max_iterations {
            return Some(LoopReason::MaxIterations {
                count: self.total_calls,
                limit: self.max_iterations,
            });
        }

        // Canonicalize args to a string for comparison.
        let args_key = args.to_string();
        let key = (tool_name.to_string(), args_key);

        let count = self.calls.entry(key).or_insert(0);
        *count += 1;

        if *count >= self.repeat_threshold {
            return Some(LoopReason::RepeatedCall {
                tool: tool_name.to_string(),
                count: *count,
                threshold: self.repeat_threshold,
            });
        }

        None
    }

    /// Reset the detector state.
    pub fn reset(&mut self) {
        self.calls.clear();
        self.total_calls = 0;
    }

    /// Current total call count.
    #[must_use]
    pub fn total_calls(&self) -> usize {
        self.total_calls
    }
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Reason a loop was detected.
#[derive(Debug, Clone)]
pub enum LoopReason {
    /// Global iteration limit reached.
    MaxIterations { count: usize, limit: usize },
    /// Same tool+args repeated too many times.
    RepeatedCall {
        tool: String,
        count: usize,
        threshold: usize,
    },
}

impl std::fmt::Display for LoopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaxIterations { count, limit } => {
                write!(f, "max iterations reached ({count}/{limit})")
            }
            Self::RepeatedCall {
                tool,
                count,
                threshold,
            } => {
                write!(
                    f,
                    "tool '{tool}' called with same args {count} times (threshold: {threshold})"
                )
            }
        }
    }
}
