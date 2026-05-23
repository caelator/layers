# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-preflight-strict-low-relevance
Variant: baseline
Time budget minutes: 45

## Task
Fix strict preflight validation so low-relevance or memory-only context cannot be reported as high-confidence code-heavy context. Add a regression test and run Rust gates.

## Required validation commands
- `cargo test -q cmd::preflight -- --nocapture`
- `cargo check --workspace --all-targets`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
