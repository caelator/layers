# Phase 8 Claim Report: first full local artifact set

## Result status
Not supported as a product-performance claim.

This Phase 8 execution produced a full paired local artifact set for all preregistered workflow tasks, but it intentionally did not perform independent coding-agent implementation for code-heavy tasks. Code-heavy rows are scored 0.0 for both baseline and targeted preflight. Negative controls were actually validated and targeted preflight abstained.

## Benchmark run
- run_id: 20260503T014112Z-layers-vs-baseline-proof
- repo_commit: e3e3763977238dc923e8c6c52ae25f3af76e6f09
- task_count: 31
- paired_variants: baseline vs layers_targeted_preflight
- randomized_order_seed: phase8-first-full-local-artifact-set-v1
- run_records: compare/workflow-runs.jsonl
- reports: compare/workflow-benchmark-report.json and compare/workflow-benchmark-report.md

## Blocking metrics
- Analyzer status: `not_supported`.
- Runs: 62 rows across 31 paired tasks.
- Success delta: 0.000.
- Net time saved: -80.9 ms/task.
- Token reduction ratio: -2.578.
- Missed critical context rate: 0.806.
- Negative-control abstention rate: 1.000.
- Unnecessary context injection rate: 0.000.
- Code-heavy task-solving was not executed; therefore this artifact set cannot support any claim that Layers improves real coding workflow success/time/tokens.
- Missed critical context is marked for every unexecuted code-heavy task because no implementation transcript can prove critical context was used.
- Generated artifacts are useful for protocol validation, report plumbing, negative-control abstention, and targeted-preflight packet capture only.

## Exact claim supported or not
The preregistered targeted-preflight claim is not supported by this run. This run is a Phase 8 artifact/protocol execution pass, not a publishable effectiveness benchmark.

## Secret scan
`SECRET_SCAN.md` records the artifact secret scan. No live credentials were found; token-accounting source-code hits were reviewed as false positives.

## Top failures
1. No independent baseline coding-agent implementation for code-heavy tasks.
2. No independent targeted-preflight coding-agent implementation after packet generation for code-heavy tasks.
3. Code-heavy validation commands were not run after task-specific edits because no task-specific edits were made.

## Next product fixes
1. Add an automated benchmark runner that can launch isolated coding-agent runs per task/variant in throwaway worktrees.
2. Add machine-readable transcript/run-record capture from actual agent sessions.
3. Add packet validation/inspection subcommands specifically for preflight JSON packets if the packet schema differs from context-packet validation.
4. Rerun Phase 8 with real agent execution before making any product-performance claim.
