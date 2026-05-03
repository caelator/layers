# Workflow Benchmark Agent Prompt

Task ID: fixture-valid-code-task
Variant: baseline
Time budget minutes: 20

## Task
Fix a small context routing regression and run the focused tests.

## Required validation commands
- `cargo test -q workflow_benchmark -- --nocapture`
- `cargo check --workspace --all-targets`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The task is implemented and all expected validation commands pass.
Partial success: The main behavior is implemented but at least one non-critical validation is missing or incomplete.
Failure: The behavior is not implemented, validation fails, or the agent relies on irrelevant context.
