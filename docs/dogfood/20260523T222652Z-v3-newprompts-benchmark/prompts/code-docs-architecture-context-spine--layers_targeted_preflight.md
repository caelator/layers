# Workflow Benchmark Agent Prompt

Task ID: code-docs-architecture-context-spine
Variant: layers_targeted_preflight
Time budget minutes: 25

## Task
Improve architecture documentation or CLI help to describe Layers as a local-first context compiler/context spine for coding agents, not a competing agent runtime. Keep claims evidence-gated.

## Required validation commands
- `cargo test -q tests::cli_about_positions_layers_as_context_packet_compiler -- --nocapture`
- `cargo check --workspace --all-targets`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `docs/dogfood/20260523T222652Z-v3-newprompts-benchmark/packets/code-docs-architecture-context-spine--layers_targeted_preflight.md`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
