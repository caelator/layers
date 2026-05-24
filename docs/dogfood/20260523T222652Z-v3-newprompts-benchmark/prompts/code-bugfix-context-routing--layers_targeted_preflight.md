# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-context-routing
Variant: layers_targeted_preflight
Time budget minutes: 30

## Task
Diagnose and fix a regression in Layers context routing where code-heavy queries with explicit Rust targets should request or inject targeted code context instead of falling back to memory-only context. Use tests to prove the routing behavior and run the relevant Rust validation gates.

## Required validation commands
- `cargo test -q cmd::query -- --nocapture`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `docs/dogfood/20260523T222652Z-v3-newprompts-benchmark/packets/code-bugfix-context-routing--layers_targeted_preflight.md`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The routing regression is fixed with focused tests, targeted code context is used or requested for code-heavy Rust tasks, and all listed Rust validation commands pass.
Partial success: The regression is understood and partially fixed, but validation is incomplete or the routing still needs manual review.
Failure: The routing behavior remains wrong, untested, or introduces regressions.
