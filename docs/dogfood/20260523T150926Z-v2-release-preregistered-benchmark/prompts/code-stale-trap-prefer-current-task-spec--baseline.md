# Workflow Benchmark Agent Prompt

Task ID: code-stale-trap-prefer-current-task-spec
Variant: baseline
Time budget minutes: 45

## Task
Stale-context trap: update code using the current TaskSpec validation rules, not older docs that allowed missing expected relevant files. The correct solution must inspect current schema and validator before editing.

## Required validation commands
- `cargo test -q workflow_benchmark -- --nocapture`
- `cargo run -q -- workflow-benchmark validate-tasks benchmarks/workflows/tasks --json`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
