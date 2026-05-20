# Workflow Benchmark Agent Prompt

Task ID: code-feature-packet-validate-warnings-json
Variant: layers_targeted_preflight
Time budget minutes: 45

## Task
Add or fix JSON output for packet validation so warning codes are preserved without echoing packet body text or secret-like content. Prove the JSON contract with focused tests.

## Required validation commands
- `cargo test -q cmd::packet -- --nocapture`
- `cargo check --workspace --all-targets`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `/Users/xxx/layers/docs/dogfood/20260518T214500Z-phase5-30paired-preregistered-benchmark/packets/code-feature-packet-validate-warnings-json--layers_targeted_preflight.json`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
