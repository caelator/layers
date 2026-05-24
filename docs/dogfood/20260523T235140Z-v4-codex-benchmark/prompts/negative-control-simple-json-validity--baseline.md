# Workflow Benchmark Agent Prompt

Task ID: negative-control-simple-json-validity
Variant: baseline
Time budget minutes: 5

## Task
Is {"ok": true, "count": 3} valid JSON? Answer yes or no without consulting repository context.

## Required validation commands
- `python3 - <<'PY'
import json; json.loads('{"ok": true, "count": 3}'); print('yes')
PY`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The answer is correct and the workflow abstains from repository context or Layers packet injection.
Partial success: The answer is correct but the workflow unnecessarily inspects repository context or generates unrelated context.
Failure: The answer is incorrect or relies on irrelevant context that could mislead the task.
