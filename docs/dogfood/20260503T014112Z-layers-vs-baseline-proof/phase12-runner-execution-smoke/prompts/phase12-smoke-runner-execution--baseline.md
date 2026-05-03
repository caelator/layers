# Workflow Benchmark Agent Prompt

Task ID: phase12-smoke-runner-execution
Variant: baseline
Time budget minutes: 5

## Task
Run the smoke agent command in an isolated worktree. The smoke agent should create agent-output.txt from this prompt. Do not make product-effectiveness claims from this task.

## Required validation commands
- `test -f agent-output.txt`
- `python3 -m json.tool packets/targeted-preflight/phase12-smoke-runner-execution--layers_targeted_preflight.json >/dev/null 2>&1 || test ! -e packets/targeted-preflight/phase12-smoke-runner-execution--layers_targeted_preflight.json`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The smoke command executes in an isolated worktree, writes agent-output.txt, and validation commands pass.
Partial success: The smoke command executes but an artifact or validation command is incomplete.
Failure: The smoke command does not execute, worktree isolation is absent, or required artifacts are missing.
