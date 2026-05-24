# Workflow Benchmark Agent Prompt

Task ID: code-feature-quality-abstain-low-specificity
Variant: baseline
Time budget minutes: 45

## Task
Improve quality evaluation so low-specificity context for code-heavy tasks produces an explicit abstain or needs-target signal instead of being accepted as useful context. Add focused tests.

## Required validation commands
- `cargo test -q quality::tests -- --nocapture`
- `cargo test -q cmd::preflight -- --nocapture`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
