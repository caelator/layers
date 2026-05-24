# Workflow Benchmark Agent Prompt

Task ID: code-stale-trap-query-surface-not-general-proof
Variant: baseline
Time budget minutes: 45

## Task
Stale-context trap: update benchmark reporting language so broad query evidence cannot be reused as targeted-preflight proof. Ignore stale notes implying all Layers surfaces can be pooled.

## Required validation commands
- `cargo test -q workflow_benchmark -- --nocapture`
- `cargo check --workspace --all-targets`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
