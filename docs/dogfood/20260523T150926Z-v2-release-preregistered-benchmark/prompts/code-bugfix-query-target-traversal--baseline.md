# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-query-target-traversal
Variant: baseline
Time budget minutes: 25

## Task
Ensure explicit query targets containing absolute paths or parent traversal cannot be treated as grounded repository targets. Add regression tests for absolute and .. paths.

## Required validation commands
- `cargo test -q context_packet_compiler::query_plan -- --nocapture`
- `cargo test -q cmd::query -- --nocapture`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
