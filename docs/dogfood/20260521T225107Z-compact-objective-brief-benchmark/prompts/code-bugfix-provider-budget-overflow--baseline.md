# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-provider-budget-overflow
Variant: baseline
Time budget minutes: 45

## Task
Audit provider budget accounting for saturating arithmetic and f64/u64 conversions. Add a regression test for very large token counters without panics or wraparound.

## Required validation commands
- `cargo test -q provider::accounting -- --nocapture`
- `cargo test -q -p layers-providers -- --nocapture`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
