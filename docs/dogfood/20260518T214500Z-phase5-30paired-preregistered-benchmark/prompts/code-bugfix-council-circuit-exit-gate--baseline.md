# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-council-circuit-exit-gate
Variant: baseline
Time budget minutes: 45

## Task
Diagnose and fix an edge case where the council circuit breaker exit gate could reopen too early or too late around threshold boundaries. Prove behavior with boundary tests.

## Required validation commands
- `cargo test -q council::circuit_breaker -- --nocapture`
- `cargo test -q council::topology -- --nocapture`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
