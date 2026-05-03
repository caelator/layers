# Workflow Benchmark Agent Prompt

Task ID: phase12-smoke-runner-execution
Variant: layers_targeted_preflight
Time budget minutes: 5

## Task
Run the smoke agent command in an isolated worktree. The smoke agent should create agent-output.txt from this prompt. Do not make product-effectiveness claims from this task.

## Required validation commands
- `test -f agent-output.txt`
- `python3 -m json.tool packets/targeted-preflight/phase12-smoke-runner-execution--layers_targeted_preflight.json >/dev/null 2>&1 || test ! -e packets/targeted-preflight/phase12-smoke-runner-execution--layers_targeted_preflight.json`

## Targeted preflight setup
The benchmark harness handles the Layers targeted-preflight step before agent execution; do not run additional `layers preflight` commands.
The harness-generated targeted-preflight packet artifact for this run is `/Users/xxx/layers/docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke/packets/phase12-smoke-runner-execution--layers_targeted_preflight.json`; inspect it if needed before editing files.
Use only the harness-captured targeted-preflight context for this variant; do not mix broad-query or MCP-preflight artifacts.

## Scoring reminder
Full success: The smoke command executes in an isolated worktree, writes agent-output.txt, and validation commands pass.
Partial success: The smoke command executes but an artifact or validation command is incomplete.
Failure: The smoke command does not execute, worktree isolation is absent, or required artifacts are missing.
