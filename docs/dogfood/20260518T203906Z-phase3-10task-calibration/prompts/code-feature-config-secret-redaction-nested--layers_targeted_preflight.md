# Workflow Benchmark Agent Prompt

Task ID: code-feature-config-secret-redaction-nested
Variant: layers_targeted_preflight
Time budget minutes: 45

## Task
Extend config masking so nested provider secret-like values are redacted consistently in displayed or serialized diagnostic output. Add tests proving short and long secrets are not echoed.

## Required validation commands
- `cargo test -q config::tests::mask -- --nocapture`
- `cargo check --workspace --all-targets`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `/Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/packets/code-feature-config-secret-redaction-nested--layers_targeted_preflight.json`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
