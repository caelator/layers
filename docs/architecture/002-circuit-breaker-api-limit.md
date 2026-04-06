# RFC: Circuit Breaker for Council Runs

**Status:** Draft
**Date:** 2026-04-06
**Source:** Inspired by Ralph's autonomous loop circuit breaker (frankbria/ralph-claude-code)

---

## Problem

Council deliberation runs can enter pathological states:
- Models keep deliberating without converging
- API errors cause retry loops that never terminate
- A stage keeps producing output that "looks like progress" but never reaches convergence

Currently layers has `retry_limit` but no equivalent safety net that forces stoppage after sustained non-progress.

---

## Design

### CircuitBreaker Struct

```rust
pub struct CircuitBreaker {
    /// Consecutive rounds with no material change
    consecutive_no_progress: u32,
    /// Threshold that triggers forced termination
    threshold: u32,
    /// Consecutive completion indicators (Ralph-style)
    completion_indicators: u32,
    /// Whether EXIT_SIGNAL: true was explicitly set
    exit_signal_received: bool,
}

impl CircuitBreaker {
    pub fn new(threshold: u32) -> Self;
    
    /// Called after each deliberation round.
    /// Returns true if the loop should continue, false if circuit is broken.
    pub fn record_round(&mut self, round_output: &CouncilRoundOutput) -> bool;
    
    /// Force-trip the breaker (for extreme cases)
    pub fn trip(&mut self);
    
    /// Whether the dual-condition exit gate is satisfied
    pub fn exit_gate_satisfied(&self) -> bool;
}
```

### Dual-Condition Exit Gate

Exit requires BOTH:
1. `completion_indicators >= 2` (heuristic: round output contains completion language)
2. `exit_signal_received == true` (explicit `STATUS: COMPLETE` from synthesis model)

This prevents premature exit when the council says "phase complete" but hasn't actually converged.

### Completion Indicator Detection

```rust
/// Semantic completion indicator analyzer.
/// Scans round output for natural language patterns that suggest progress.
fn detect_completion_indicators(output: &str) -> u32 {
    let mut score = 0;
    let lower = output.to_lowercase();
    
    // Strong signals
    if lower.contains("status: complete") || lower.contains("exit_signal: true") {
        score += 2;
    }
    // Moderate signals
    if lower.contains("phase complete") || lower.contains("converged") 
       || lower.contains("deliberation settled") {
        score += 1;
    }
    // Weak signals
    if lower.contains("moving to next") || lower.contains("conclusion reached") {
        score += 1;
    }
    
    score
}
```

### Integration with CouncilRun

In `src/council/mod.rs`, add a circuit breaker field to `CouncilRun`:

```rust
let mut circuit_breaker = CircuitBreaker::new(
    env::var("COUNCIL_CIRCUIT_BREAKER_THRESHOLD")
        .unwrap_or_else(|_| "5".to_string())
        .parse()
        .unwrap_or(5)
);

for stage in &stages {
    let outcome = execute_stage(&stage, &mut ctx)?;
    
    let indicators = detect_completion_indicators(&outcome.output);
    circuit_breaker.record_indicators(indicators);
    
    if circuit_breaker.should_trip() {
        return Err(anyhow::anyhow!(
            "Circuit breaker tripped: {} consecutive non-progress rounds",
            circuit_breaker.consecutive_no_progress
        ));
    }
    
    if circuit_breaker.exit_gate_satisfied() && outcome.is_converged() {
        break;
    }
}
```

### Environment Variables

| Var | Default | Description |
|-----|---------|-------------|
| `COUNCIL_CIRCUIT_BREAKER_THRESHOLD` | `5` | Max consecutive no-progress rounds before forced stop |
| `COUNCIL_COMPLETION_INDICATOR_MIN` | `2` | Min indicators needed for exit gate |

---

## API Limit Detection (Three-Layer)

Complement the circuit breaker with explicit API limit detection.

### Ralph's Three Layers

1. **Timeout guard**: exit code 124 → API limit
2. **Structural JSON**: `rate_limit_event` in response
3. **Text fallback**: patterns like "rate limit", "too many requests" in stderr

```rust
#[derive(Debug)]
pub enum ApiLimitReason {
    Timeout,
    RateLimitEvent,
    TextPatternMatch,
}

pub fn detect_api_limit(err: &str, exit_code: Option<u32>) -> Option<ApiLimitReason> {
    // Layer 1: timeout exit code
    if exit_code == Some(124) {
        return Some(ApiLimitReason::Timeout);
    }
    
    // Layer 2: structural JSON
    if err.contains("rate_limit_event") {
        return Some(ApiLimitReason::RateLimitEvent);
    }
    
    // Layer 3: text patterns
    let lower = err.to_lowercase();
    if lower.contains("rate limit") || lower.contains("too many requests") 
       || lower.contains("429") || lower.contains("api limit") {
        return Some(ApiLimitReason::TextPatternMatch);
    }
    
    None
}
```

---

## Out of Scope

- Progress heuristics beyond simple keyword matching (save NLP for later)
- Circuit breaker persistence across sessions (each run starts fresh)
- Auto-retry on API limit (that belongs in the retry logic, not the breaker)
