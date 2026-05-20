# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-router-historical-code-intent
Variant: baseline
Time budget minutes: 45

## Task
Fix routing so historical or prior-decision questions that mention code terms remain memory-eligible unless they clearly request edit/debug/action work. Add benchmark-style routing tests.

## Required validation commands
- `cargo test -q router::tests -- --nocapture`
- `cargo test -q cmd::query -- --nocapture`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
