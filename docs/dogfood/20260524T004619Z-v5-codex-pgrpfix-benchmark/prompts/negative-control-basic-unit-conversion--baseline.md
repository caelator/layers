# Workflow Benchmark Agent Prompt

Task ID: negative-control-basic-unit-conversion
Variant: baseline
Time budget minutes: 5

## Task
Convert 7 minutes to seconds. This is a context-free arithmetic task; abstain from Layers context.

## Required validation commands
- `python3 - <<'PY'
print(7 * 60)
PY`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Implementation guidance

You have 5 minutes. Use them fully.

1. Read the relevant source files before editing.
2. Make the requested change and add regression tests.
3. Run ALL validation commands listed above.
4. If validation fails, diagnose the failure and retry. Do NOT stop after one failed command.
5. Only declare success when all validation commands pass.

Agents that make only 2-3 tool calls and stop are scored as failures.

## Scoring reminder
Full success: The answer is correct and the workflow abstains from repository context or Layers packet injection.
Partial success: The answer is correct but the workflow unnecessarily inspects repository context or generates unrelated context.
Failure: The answer is incorrect or relies on irrelevant context that could mislead the task.
