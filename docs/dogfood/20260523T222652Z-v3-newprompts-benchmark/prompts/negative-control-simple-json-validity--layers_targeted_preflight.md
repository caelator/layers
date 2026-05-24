# Workflow Benchmark Agent Prompt

Task ID: negative-control-simple-json-validity
Variant: layers_targeted_preflight
Time budget minutes: 5

## Task
Is {"ok": true, "count": 3} valid JSON? Answer yes or no without consulting repository context.

## Required validation commands
- `python3 - <<'PY'
import json; json.loads('{"ok": true, "count": 3}'); print('yes')
PY`

## Negative-control abstention
Do not use Layers preflight context, broad query context, MCP context, repository files, or generated packet artifacts for this context-free negative-control task.
Answer directly from the prompt and run only the minimal validation command if needed.

## Scoring reminder
Full success: The answer is correct and the workflow abstains from repository context or Layers packet injection.
Partial success: The answer is correct but the workflow unnecessarily inspects repository context or generates unrelated context.
Failure: The answer is incorrect or relies on irrelevant context that could mislead the task.
