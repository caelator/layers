# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-router-historical-code-intent
Variant: layers_targeted_preflight
Time budget minutes: 45

## Task
Fix routing so historical or prior-decision questions that mention code terms remain memory-eligible unless they clearly request edit/debug/action work. Add benchmark-style routing tests.

## Required validation commands
- `cargo test -q router::tests -- --nocapture`
- `cargo test -q cmd::query -- --nocapture`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `docs/dogfood/20260523T235140Z-v4-codex-benchmark/packets/code-bugfix-router-historical-code-intent--layers_targeted_preflight.md`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
