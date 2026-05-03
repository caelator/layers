# Workflow Benchmark Agent Prompt

Task ID: fixture-valid-code-task
Variant: layers_targeted_preflight
Time budget minutes: 20

## Task
Fix a small context routing regression and run the focused tests.

## Required validation commands
- `cargo test -q workflow_benchmark -- --nocapture`
- `cargo check --workspace --all-targets`

## Targeted preflight setup
Run `layers preflight --no-audit --json --strict` before implementation.
Save the preflight JSON packet to `docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase11-runner-plan/packets/fixture-valid-code-task--layers_targeted_preflight.json` before editing files.
Use only targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The task is implemented and all expected validation commands pass.
Partial success: The main behavior is implemented but at least one non-critical validation is missing or incomplete.
Failure: The behavior is not implemented, validation fails, or the agent relies on irrelevant context.
