# Workflow Benchmark Agent Prompt

Task ID: code-docs-architecture-context-spine
Variant: baseline
Time budget minutes: 25

## Task
Improve architecture documentation or CLI help to describe Layers as a local-first context compiler/context spine for coding agents, not a competing agent runtime. Keep claims evidence-gated.

## Required validation commands
- `cargo test -q tests::cli_about_positions_layers_as_context_packet_compiler -- --nocapture`
- `cargo check --workspace --all-targets`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
