# Workflow Benchmark Agent Prompt

Task ID: negative-control-basic-unit-conversion
Variant: layers_targeted_preflight
Time budget minutes: 5

## Task
Convert 7 minutes to seconds. This is a context-free arithmetic task; abstain from Layers context.

## Required validation commands
- `python3 - <<'PY'
print(7 * 60)
PY`

## Negative-control abstention
Do not use Layers preflight context, broad query context, MCP context, repository files, or generated packet artifacts for this context-free negative-control task.
Answer directly from the prompt and run only the minimal validation command if needed.

## Scoring reminder
Full success: The answer is correct and the workflow abstains from repository context or Layers packet injection.
Partial success: The answer is correct but the workflow unnecessarily inspects repository context or generates unrelated context.
Failure: The answer is incorrect or relies on irrelevant context that could mislead the task.
