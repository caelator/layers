# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-provider-budget-overflow
Variant: layers_targeted_preflight
Time budget minutes: 45

## Task
Audit provider budget accounting for saturating arithmetic and f64/u64 conversions. Add a regression test for very large token counters without panics or wraparound.

## Required validation commands
- `cargo test -q provider::accounting -- --nocapture`
- `cargo test -q -p layers-providers -- --nocapture`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `docs/dogfood/20260521T225107Z-compact-objective-brief-benchmark/packets/code-bugfix-provider-budget-overflow--layers_targeted_preflight.md`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
