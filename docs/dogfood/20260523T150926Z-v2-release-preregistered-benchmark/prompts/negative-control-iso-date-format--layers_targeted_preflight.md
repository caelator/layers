# Workflow Benchmark Agent Prompt

Task ID: negative-control-iso-date-format
Variant: layers_targeted_preflight
Time budget minutes: 5

## Task
Rewrite the provided date March 7, 2026 as ISO 8601 date format. No repository context is relevant.

## Required validation commands
- `python3 - <<'PY'
print('2026-03-07')
PY`

## Negative-control abstention
Do not use Layers preflight context, broad query context, MCP context, repository files, or generated packet artifacts for this context-free negative-control task.
Answer directly from the prompt and run only the minimal validation command if needed.

## Scoring reminder
Full success: The answer is correct and the workflow abstains from repository context or Layers packet injection.
Partial success: The answer is correct but the workflow unnecessarily inspects repository context or generates unrelated context.
Failure: The answer is incorrect or relies on irrelevant context that could mislead the task.
