# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-context-routing
Variant: baseline
Time budget minutes: 30

## Task
Diagnose and fix a regression in Layers context routing where code-heavy queries with explicit Rust targets should request or inject targeted code context instead of falling back to memory-only context. Use tests to prove the routing behavior and run the relevant Rust validation gates.

## Required validation commands
- `cargo test -q cmd::query -- --nocapture`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Implementation guidance

You have 30 minutes. Use them fully.

1. Read the relevant source files before editing.
2. Make the requested change and add regression tests.
3. Run ALL validation commands listed above.
4. If validation fails, diagnose the failure and retry. Do NOT stop after one failed command.
5. Only declare success when all validation commands pass.

Agents that make only 2-3 tool calls and stop are scored as failures.

## Scoring reminder
Full success: The routing regression is fixed with focused tests, targeted code context is used or requested for code-heavy Rust tasks, and all listed Rust validation commands pass.
Partial success: The regression is understood and partially fixed, but validation is incomplete or the routing still needs manual review.
Failure: The routing behavior remains wrong, untested, or introduces regressions.
