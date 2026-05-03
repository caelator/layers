# Phase 10 product-fix loop: analyzer claim-gate alignment

## Scope

This Phase 10 pass addressed the Phase 9 analyzer-gate mismatch before any further benchmark claim is allowed to rely on analyzer status.

It did not rerun independent coding-agent implementations. The Phase 8 artifact set remains useful for protocol/report plumbing and remains `not_supported` as a product-performance claim.

## Product fix implemented

The workflow benchmark analyzer now mirrors the preregistered `benchmarks/workflows/CLAIM_GATES.md` sample and safety gates in its default claim thresholds:

- `min_paired_tasks: 30`
- `min_code_heavy_paired_tasks: 20`
- `min_negative_control_paired_tasks: 5`
- `min_token_reduction_ratio: 0.20`
- `min_negative_control_abstention_rate: 0.95`
- `max_unnecessary_context_injection_rate: 0.05`
- `max_context_caused_regression_rate: 0.0`

The analyzer JSON claim report now also emits:

- `code_heavy_paired_task_count`
- `negative_control_paired_task_count`

Sample-size failures are treated as inconclusive unless a hard metric gate fails. Hard metric failures still produce `not_supported`.

## Regression tests added

`src/cmd/workflow_benchmark.rs` now includes tests that require:

- analyzer default thresholds to match preregistered gates,
- the analyzer to surface insufficient paired/code-heavy sample size as uncertainty,
- existing permissive-threshold claim tests to keep exercising metric-gate behavior without being distorted by preregistered production sample-size gates.

## Re-analysis of Phase 8/9 artifact set

The existing run records were re-analyzed with the fixed analyzer and the report artifacts were regenerated:

- `compare/workflow-benchmark-report.json`
- `compare/workflow-benchmark-report.md`

Updated analyzer result:

- status: `not_supported`
- paired tasks: 31
- code-heavy paired tasks: 24
- negative-control paired tasks: 6
- blocking metrics: `net_time_saved_ms`, `token_reduction_ratio`, `missed_critical_context_rate`

The previous Phase 9 warning about analyzer sample thresholds being weaker than `CLAIM_GATES.md` is resolved. The product-performance claim remains blocked because the artifact set still did not execute independent code-heavy coding-agent task-solving and still fails the hard effect/context gates.

## Remaining Phase 10/product-loop work

Still required before any real effectiveness claim:

1. Build the automated isolated coding-agent benchmark runner.
2. Capture real transcripts and run records from actual task-solving runs.
3. Reset throwaway worktrees between variants.
4. Enforce identical model/tool/time budgets.
5. Run task-specific validation commands after edits.
6. Clean up negative-control token accounting for abstained targeted-preflight rows.
7. Rerun the benchmark and repeat independent artifact/static review.
