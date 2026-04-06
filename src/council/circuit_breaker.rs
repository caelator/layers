//! Circuit breaker for council runs.
//!
//! Prevents infinite loops by tracking consecutive no-progress rounds and
//! force-tripping when a threshold is exceeded. Also provides API limit detection
//! and a dual-condition exit gate.

use std::env;

/// Minimum number of completion indicators required for exit gate.
const DEFAULT_COMPLETION_INDICATOR_MIN: u32 = 2;

/// Default threshold for consecutive no-progress rounds before tripping.
const DEFAULT_CIRCUIT_BREAKER_THRESHOLD: u32 = 5;

/// Reason for API limit detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ApiLimitReason {
    /// Process exited with timeout code (124).
    Timeout,
    /// Structured rate limit event detected in error output.
    RateLimitEvent,
    /// Text pattern matching rate limit keywords.
    TextPatternMatch,
}

/// Circuit breaker state for council run iteration control.
pub struct CircuitBreaker {
    /// Consecutive rounds with no completion indicators.
    consecutive_no_progress: u32,
    /// Threshold at which the circuit trips.
    threshold: u32,
    /// Accumulated completion indicators from outputs.
    completion_indicators: u32,
    /// Whether an explicit exit signal has been received.
    exit_signal_received: bool,
    /// Minimum completion indicators required for exit gate.
    #[allow(dead_code)]
    completion_indicator_min: u32,
}

#[allow(dead_code)]
impl CircuitBreaker {
    /// Create a new circuit breaker with the given threshold.
    pub fn new(threshold: u32) -> Self {
        let completion_indicator_min = env::var("COUNCIL_COMPLETION_INDICATOR_MIN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_COMPLETION_INDICATOR_MIN);

        Self {
            consecutive_no_progress: 0,
            threshold,
            completion_indicators: 0,
            exit_signal_received: false,
            completion_indicator_min,
        }
    }

    /// Record a round's output and return whether the circuit is still open.
    ///
    /// Returns `false` when the circuit has tripped (threshold exceeded).
    pub fn record_round(&mut self, round_output: &str) -> bool {
        let indicators = detect_completion_indicators(round_output);

        if indicators > 0 {
            self.consecutive_no_progress = 0;
            self.completion_indicators += indicators;

            // Check for exit signal
            if round_output.contains("exit_signal: true") {
                self.exit_signal_received = true;
            }
        } else {
            self.consecutive_no_progress += 1;
        }

        // Trip if consecutive no-progress exceeds threshold
        if self.consecutive_no_progress > self.threshold {
            return false;
        }

        true
    }

    /// Force-trip the circuit breaker.
    pub fn trip(&mut self) {
        // Set consecutive_no_progress beyond threshold to force trip
        self.consecutive_no_progress = self.threshold.saturating_add(1);
    }

    /// Check if the exit gate is satisfied.
    ///
    /// The exit gate requires BOTH:
    /// - `completion_indicators >= completion_indicator_min` (default 2)
    /// - `exit_signal_received == true`
    pub fn exit_gate_satisfied(&self) -> bool {
        self.completion_indicators >= self.completion_indicator_min
            && self.exit_signal_received
    }

    /// Returns the current consecutive no-progress count.
    pub fn consecutive_no_progress(&self) -> u32 {
        self.consecutive_no_progress
    }

    /// Returns the current completion indicator count.
    #[allow(dead_code)]
    pub fn completion_indicators(&self) -> u32 {
        self.completion_indicators
    }

    /// Returns whether an exit signal has been received.
    #[allow(dead_code)]
    pub fn exit_signal_received(&self) -> bool {
        self.exit_signal_received
    }

    /// Returns whether the circuit has tripped.
    pub fn is_tripped(&self) -> bool {
        self.consecutive_no_progress > self.threshold
    }
}

/// Detect completion indicators in stage output.
///
/// Scoring:
/// - +2: "status: complete" or `exit_signal: true`
/// - +1: "phase complete" or "converged" or "deliberation settled"
/// - +1: "moving to next" or "conclusion reached"
pub fn detect_completion_indicators(output: &str) -> u32 {
    let mut score = 0u32;
    let lower = output.to_lowercase();

    // +2 indicators
    if lower.contains("status: complete") || lower.contains("exit_signal: true") {
        score += 2;
    }

    // +1 indicators (first group)
    if lower.contains("phase complete")
        || lower.contains("converged")
        || lower.contains("deliberation settled")
    {
        score += 1;
    }

    // +1 indicators (second group)
    if lower.contains("moving to next") || lower.contains("conclusion reached") {
        score += 1;
    }

    score
}

/// Detect API limit conditions from error output and exit code.
///
/// Three-layer detection:
/// 1. exit code 124 → Timeout
/// 2. `rate_limit_event` in err → `RateLimitEvent`
/// 3. "rate limit" / "too many requests" / "429" / "api limit" → `TextPatternMatch`
#[allow(dead_code)]
pub fn detect_api_limit(err: &str, exit_code: Option<u32>) -> Option<ApiLimitReason> {
    // Layer 1: Timeout exit code
    if exit_code == Some(124) {
        return Some(ApiLimitReason::Timeout);
    }

    let lower = err.to_lowercase();

    // Layer 2: Structured rate limit event
    if lower.contains("rate_limit_event") {
        return Some(ApiLimitReason::RateLimitEvent);
    }

    // Layer 3: Text pattern matching
    if lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("429")
        || lower.contains("api limit")
    {
        return Some(ApiLimitReason::TextPatternMatch);
    }

    None
}

/// Create a circuit breaker from environment variables or defaults.
pub fn from_env() -> CircuitBreaker {
    let threshold = env::var("COUNCIL_CIRCUIT_BREAKER_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_CIRCUIT_BREAKER_THRESHOLD);

    CircuitBreaker::new(threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_trips_after_threshold() {
        let threshold = 3;
        let mut cb = CircuitBreaker::new(threshold);

        // Record rounds with no progress
        for i in 0..threshold {
            let should_continue = cb.record_round("no progress here");
            assert!(
                should_continue,
                "circuit should still be open after {} no-progress rounds",
                i + 1
            );
            assert!(
                !cb.is_tripped(),
                "circuit should not be tripped after {} rounds",
                i + 1
            );
        }

        // Next round should trip
        let should_continue = cb.record_round("still no progress");
        assert!(
            !should_continue,
            "circuit should trip after exceeding threshold"
        );
        assert!(cb.is_tripped(), "circuit should be marked as tripped");
    }

    #[test]
    fn test_circuit_breaker_exit_gate_requires_both_conditions() {
        let mut cb = CircuitBreaker::new(5);

        // Neither condition met
        assert!(
            !cb.exit_gate_satisfied(),
            "exit gate should be false when neither condition is met"
        );

        // Only completion indicators (without exit signal)
        cb.completion_indicators = 3;
        assert!(
            !cb.exit_gate_satisfied(),
            "exit gate should be false when only completion indicators present"
        );

        // Only exit signal (without sufficient completion indicators)
        cb.completion_indicators = 0;
        cb.exit_signal_received = true;
        assert!(
            !cb.exit_gate_satisfied(),
            "exit gate should be false when only exit signal present"
        );

        // Both conditions met
        cb.completion_indicators = 2;
        cb.exit_signal_received = true;
        assert!(
            cb.exit_gate_satisfied(),
            "exit gate should be true when both conditions are met"
        );
    }

    #[test]
    fn test_completion_indicator_scoring() {
        // +2 indicators
        assert_eq!(
            detect_completion_indicators("status: complete"),
            2,
            "status: complete should give +2"
        );
        assert_eq!(
            detect_completion_indicators("exit_signal: true"),
            2,
            "exit_signal: true should give +2"
        );
        assert_eq!(
            detect_completion_indicators("STATUS: COMPLETE"),
            2,
            "case insensitive"
        );

        // +1 indicators (first group)
        assert_eq!(
            detect_completion_indicators("phase complete"),
            1,
            "phase complete should give +1"
        );
        assert_eq!(
            detect_completion_indicators("converged"),
            1,
            "converged should give +1"
        );
        assert_eq!(
            detect_completion_indicators("deliberation settled"),
            1,
            "deliberation settled should give +1"
        );

        // +1 indicators (second group)
        assert_eq!(
            detect_completion_indicators("moving to next"),
            1,
            "moving to next should give +1"
        );
        assert_eq!(
            detect_completion_indicators("conclusion reached"),
            1,
            "conclusion reached should give +1"
        );

        // Combined scoring
        assert_eq!(
            detect_completion_indicators("status: complete and converged"),
            3,
            "status: complete (+2) + converged (+1) should give 3"
        );
        assert_eq!(
            detect_completion_indicators("exit_signal: true, phase complete, moving to next"),
            4,
            "exit_signal (+2) + phase complete (+1) + moving to next (+1) should give 4"
        );

        // No indicators
        assert_eq!(
            detect_completion_indicators("random output"),
            0,
            "random output should give 0"
        );
    }

    #[test]
    fn test_api_limit_three_layer_detection() {
        // Layer 1: Timeout exit code
        assert_eq!(
            detect_api_limit("some error", Some(124)),
            Some(ApiLimitReason::Timeout),
            "exit code 124 should detect Timeout"
        );
        assert_eq!(
            detect_api_limit("rate_limit_event detected", Some(124)),
            Some(ApiLimitReason::Timeout),
            "exit code 124 takes precedence"
        );

        // Layer 2: Structured rate limit event
        assert_eq!(
            detect_api_limit("rate_limit_event occurred", None),
            Some(ApiLimitReason::RateLimitEvent),
            "rate_limit_event should detect RateLimitEvent"
        );
        assert_eq!(
            detect_api_limit("RATE_LIMIT_EVENT in JSON", None),
            Some(ApiLimitReason::RateLimitEvent),
            "case insensitive"
        );

        // Layer 3: Text pattern matching
        assert_eq!(
            detect_api_limit("rate limit exceeded", None),
            Some(ApiLimitReason::TextPatternMatch),
            "rate limit should detect TextPatternMatch"
        );
        assert_eq!(
            detect_api_limit("too many requests", None),
            Some(ApiLimitReason::TextPatternMatch),
            "too many requests should detect TextPatternMatch"
        );
        assert_eq!(
            detect_api_limit("error 429", None),
            Some(ApiLimitReason::TextPatternMatch),
            "429 should detect TextPatternMatch"
        );
        assert_eq!(
            detect_api_limit("api limit reached", None),
            Some(ApiLimitReason::TextPatternMatch),
            "api limit should detect TextPatternMatch"
        );

        // No API limit
        assert_eq!(
            detect_api_limit("normal error", None),
            None,
            "normal error should return None"
        );
    }

    #[test]
    fn test_record_round_updates_state_correctly() {
        let mut cb = CircuitBreaker::new(5);

        // Round with completion indicators
        cb.record_round("status: complete");
        assert_eq!(cb.completion_indicators, 2);
        assert_eq!(cb.consecutive_no_progress, 0);
        assert!(!cb.exit_signal_received);

        // Round with exit signal
        cb.record_round("exit_signal: true");
        assert_eq!(cb.completion_indicators, 4); // +2 from previous +2 from this
        assert!(cb.exit_signal_received);

        // Round with no progress
        cb.record_round("no progress");
        assert_eq!(cb.consecutive_no_progress, 1);
    }

    #[test]
    fn test_trip_forcefully_opens_circuit() {
        let mut cb = CircuitBreaker::new(5);
        assert!(!cb.is_tripped());

        cb.trip();
        assert!(cb.is_tripped());
        assert!(!cb.record_round("any output"));
    }

    #[test]
    fn test_exit_gate_with_default_min_value() {
        let cb = CircuitBreaker::new(5);
        // Default completion_indicator_min is 2
        assert_eq!(cb.completion_indicator_min, 2);
    }
}
