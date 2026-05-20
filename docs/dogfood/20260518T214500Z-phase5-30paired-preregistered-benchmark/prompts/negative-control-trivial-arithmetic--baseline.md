# Workflow Benchmark Agent Prompt

Task ID: negative-control-trivial-arithmetic
Variant: baseline
Time budget minutes: 30

## Task
Answer the arithmetic question 17 + 25 without consulting repository context or injecting Layers context.

## Required validation commands
- `python3 - <<'PY'
print(17 + 25)
PY`

## Baseline isolation
Do not run Layers commands, inspect Layers packet artifacts, or use preflight-generated context.
Work from the repository and the task prompt only so this run remains a clean no-Layers baseline.

## Scoring reminder
Full success: The answer is correct and no repository, memory, or generated context packet is needed.
Partial success: The answer is correct but the workflow unnecessarily inspects or injects unrelated context.
Failure: The answer is incorrect or relies on irrelevant context that could mislead the task.
