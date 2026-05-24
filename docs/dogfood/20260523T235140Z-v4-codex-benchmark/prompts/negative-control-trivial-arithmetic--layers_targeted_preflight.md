# Workflow Benchmark Agent Prompt

Task ID: negative-control-trivial-arithmetic
Variant: layers_targeted_preflight
Time budget minutes: 30

## Task
Answer the arithmetic question 17 + 25 without consulting repository context or injecting Layers context.

## Required validation commands
- `python3 - <<'PY'
print(17 + 25)
PY`

## Negative-control abstention
Do not use Layers preflight context, broad query context, MCP context, repository files, or generated packet artifacts for this context-free negative-control task.
Answer directly from the prompt and run only the minimal validation command if needed.

## Scoring reminder
Full success: The answer is correct and no repository, memory, or generated context packet is needed.
Partial success: The answer is correct but the workflow unnecessarily inspects or injects unrelated context.
Failure: The answer is incorrect or relies on irrelevant context that could mislead the task.
