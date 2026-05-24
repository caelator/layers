# Workflow Benchmark Agent Prompt

Task ID: code-bugfix-runtime-queue-starvation
Variant: layers_targeted_preflight
Time budget minutes: 75

## Task
Diagnose and fix a runtime queue fairness regression where critical work can starve standard work beyond the configured ratio. Add a deterministic regression test.

## Required validation commands
- `cargo test -q -p layers-runtime -- --nocapture`
- `cargo test -q critical_path -- --nocapture`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `docs/dogfood/20260524T004619Z-v5-codex-pgrpfix-benchmark/packets/code-bugfix-runtime-queue-starvation--layers_targeted_preflight.md`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Implementation guidance

You have 75 minutes. Use them fully.

1. Read the relevant source files before editing.
2. Make the requested change and add regression tests.
3. Run ALL validation commands listed above.
4. If validation fails, diagnose the failure and retry. Do NOT stop after one failed command.
5. Only declare success when all validation commands pass.

Agents that make only 2-3 tool calls and stop are scored as failures.

## Scoring reminder
Full success: The requested repository change is implemented with focused regression coverage and all listed validation commands pass.
Partial success: The main behavior is implemented but validation is incomplete, coverage is weak, or minor follow-up is required.
Failure: The requested behavior remains missing, untested, or introduces regressions.
