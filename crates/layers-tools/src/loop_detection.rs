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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_loop_on_varied_calls() {
        let mut det = LoopDetector::new();
        assert!(det
            .record("read", &serde_json::json!({ "path": "/a" }))
            .is_none());
        assert!(det
            .record("read", &serde_json::json!({ "path": "/b" }))
            .is_none());
        assert!(det
            .record("write", &serde_json::json!({ "path": "/c" }))
            .is_none());
    }

    #[test]
    fn repeated_same_call_detected() {
        let mut det = LoopDetector::new().with_repeat_threshold(3);
        let args = serde_json::json!({ "path": "/same" });
        assert!(det.record("read", &args).is_none());
        assert!(det.record("read", &args).is_none());
        let reason = det.record("read", &args);
        assert!(reason.is_some());
        match reason {
            Some(LoopReason::RepeatedCall { tool, count, threshold }) => {
                assert_eq!(tool, "read");
                assert_eq!(count, 3);
                assert_eq!(threshold, 3);
            }
            other => panic!("Expected RepeatedCall, got {other:?}"),
        }
    }

    #[test]
    fn max_iterations_enforced() {
        let mut det = LoopDetector::new().with_max_iterations(5);
        for i in 0..5 {
            let result = det.record(
                &format!("tool_{i}"),
                &serde_json::json!({ "i": i }),
            );
            if i >= 4 {
                assert!(result.is_some());
                match result {
                    Some(LoopReason::MaxIterations { count, limit }) => {
                        assert_eq!(count, 5);
                        assert_eq!(limit, 5);
                    }
                    _ => panic!("Expected MaxIterations"),
                }
            }
        }
    }

    #[test]
    fn reset_clears_state() {
        let mut det = LoopDetector::new().with_repeat_threshold(2);
        let args = serde_json::json!({ "x": 1 });
        det.record("tool", &args);
        assert!(det.record("tool", &args).is_some());
        det.reset();
        assert_eq!(det.total_calls(), 0);
        // Should not detect loop after reset.
        assert!(det.record("tool", &args).is_none());
    }

    #[test]
    fn param_normalization_deterministic() {
        let mut det = LoopDetector::new().with_repeat_threshold(2);
        // serde_json serializes with sorted keys by default, so
        // different insertion order produces the same string.
        let a = serde_json::json!({ "a": 1, "b": 2 });
        let b = serde_json::json!({ "b": 2, "a": 1 });
        det.record("tool", &a);
        // These produce the same canonical string, so loop IS detected.
        assert!(det.record("tool", &b).is_some());
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
