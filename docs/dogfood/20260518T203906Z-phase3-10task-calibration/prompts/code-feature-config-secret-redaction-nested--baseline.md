# Workflow Benchmark Agent Prompt

Task ID: code-feature-config-secret-redaction-nested
Variant: baseline
Time budget minutes: 45

## Task
Extend config masking so nested provider secret-like values are redacted consistently in displayed or serialized diagnostic output. Add tests proving short and long secrets are not echoed.

## Required validation commands
- `cargo test -q config::tests::mask -- --nocapture`
- `cargo check --workspace --all-targets`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
