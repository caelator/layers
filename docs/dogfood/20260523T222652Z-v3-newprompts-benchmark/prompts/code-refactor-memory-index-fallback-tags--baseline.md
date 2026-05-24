# Workflow Benchmark Agent Prompt

Task ID: code-refactor-memory-index-fallback-tags
Variant: baseline
Time budget minutes: 45

## Task
Refactor memory index retrieval fallback tags so UC unavailable, timeout, and low-result cases are reported consistently without changing successful retrieval output.

## Required validation commands
- `cargo test -q uc::tests -- --nocapture`
- `cargo test -q memory_index -- --nocapture`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
