# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-telemetry-malformed-jsonl-skip
Variant: baseline
Time budget minutes: 25

## Task
Harden telemetry JSONL loading so malformed lines are skipped or reported without panics and without losing valid events before or after the bad line. Add regression tests.

## Required validation commands
- `cargo test -q plugins::telemetry -- --nocapture`
- `cargo check --workspace --all-targets`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
