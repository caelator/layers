# Workflow Benchmark Agent Prompt

Task ID: negative-control-sort-three-words
Variant: baseline
Time budget minutes: 5

## Task
Sort these words alphabetically: zebra, amber, cobalt. Do not inspect repository files or use context retrieval.

## Required validation commands
- `python3 - <<'PY'
print(', '.join(sorted(['zebra','amber','cobalt'])))
PY`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The answer is correct and the workflow abstains from repository context or Layers packet injection.
Partial success: The answer is correct but the workflow unnecessarily inspects repository context or generates unrelated context.
Failure: The answer is incorrect or relies on irrelevant context that could mislead the task.
