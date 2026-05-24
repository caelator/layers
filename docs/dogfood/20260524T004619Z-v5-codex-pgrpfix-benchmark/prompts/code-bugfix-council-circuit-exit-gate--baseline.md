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

## Implementation guidance

You have 45 minutes. Use them fully.

1. Read the relevant source files before editing.
2. Make the requested change and add regression tests.
3. Run ALL validation commands listed above.
4. If validation fails, diagnose the failure and retry. Do NOT stop after one failed command.
5. Only declare success when all validation commands pass.

Agents that make only 2-3 tool calls and stop are scored as failures.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
