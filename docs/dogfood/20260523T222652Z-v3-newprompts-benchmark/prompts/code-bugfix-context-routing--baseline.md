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

## Scoring reminder
Full success: The routing regression is fixed with focused tests, targeted code context is used or requested for code-heavy Rust tasks, and all listed Rust validation commands pass.
Partial success: The regression is understood and partially fixed, but validation is incomplete or the routing still needs manual review.
Failure: The routing behavior remains wrong, untested, or introduces regressions.
