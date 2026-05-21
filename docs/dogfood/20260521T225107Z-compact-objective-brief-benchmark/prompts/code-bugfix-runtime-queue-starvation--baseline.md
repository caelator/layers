# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-runtime-queue-starvation
Variant: baseline
Time budget minutes: 75

## Task
Diagnose and fix a runtime queue fairness regression where critical work can starve standard work beyond the configured ratio. Add a deterministic regression test.

## Required validation commands
- `cargo test -q -p layers-runtime -- --nocapture`
- `cargo test -q critical_path -- --nocapture`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
