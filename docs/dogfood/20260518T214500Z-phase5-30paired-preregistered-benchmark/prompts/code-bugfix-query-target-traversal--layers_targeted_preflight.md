# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-query-target-traversal
Variant: layers_targeted_preflight
Time budget minutes: 25

## Task
Ensure explicit query targets containing absolute paths or parent traversal cannot be treated as grounded repository targets. Add regression tests for absolute and .. paths.

## Required validation commands
- `cargo test -q context_packet_compiler::query_plan -- --nocapture`
- `cargo test -q cmd::query -- --nocapture`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `/Users/xxx/layers/docs/dogfood/20260518T214500Z-phase5-30paired-preregistered-benchmark/packets/code-bugfix-query-target-traversal--layers_targeted_preflight.json`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
