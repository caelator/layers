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

## Scoring reminder
Full success: The answer is correct and the workflow abstains from repository context or Layers packet injection.
Partial success: The answer is correct but the workflow unnecessarily inspects repository context or generates unrelated context.
Failure: The answer is incorrect or relies on irrelevant context that could mislead the task.
