# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-telemetry-malformed-jsonl-skip
Variant: layers_targeted_preflight
Time budget minutes: 25

## Task
Harden telemetry JSONL loading so malformed lines are skipped or reported without panics and without losing valid events before or after the bad line. Add regression tests.

## Required validation commands
- `cargo test -q plugins::telemetry -- --nocapture`
- `cargo check --workspace --all-targets`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/packets/code-bugfix-telemetry-malformed-jsonl-skip--layers_targeted_preflight.md`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
