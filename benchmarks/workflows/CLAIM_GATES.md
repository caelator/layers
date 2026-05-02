# Layers Workflow Benchmark Claim Gates v1

This file preregisters the first proof target for Layers-vs-baseline workflow benchmarks. Do not loosen these gates after seeing benchmark results. If a future change edits this file, the benchmark run must clearly state which gate version it used.

## Preregistered claim

Target claim:

> On code-heavy repo tasks, targeted Layers preflight maintains or improves verified success compared with no Layers while reducing context/time cost after overhead and avoiding stale or unnecessary context injection.

This does not claim that broad `layers query` is better than no Layers. Broad query, targeted preflight, and MCP preflight must be reported as separate surfaces.

## Primary endpoint

The primary endpoint is verified task success.

A claim can be supported only when the paired success delta for the claim variant is non-negative:

```text
success_delta = layers_targeted_preflight_success_rate - baseline_success_rate
min_success_delta: 0.0
```

Token reduction, time savings, tool-call reduction, or subjective context quality cannot compensate for a negative success delta.

## Minimum sample sizes

The first supported claim requires all of these:

```text
min_paired_tasks: 30
min_code_heavy_paired_tasks: 20
min_negative_control_paired_tasks: 5
```

Runs below these thresholds are `inconclusive` unless a hard blocker makes the result `not_supported`.

## Secondary endpoints

The targeted-preflight claim must also meet these cost/usefulness gates after accounting for Layers overhead:

```text
min_token_reduction_ratio: 0.20
min_time_saved_ms: 0.0
```

Report these secondary metrics even when the claim is not supported:

- wall-clock time delta
- total token delta
- tool-call delta
- failed-command delta
- human-intervention delta
- context relevance/completeness/specificity/freshness/grounding/concision/noise scores
- Layers overhead in milliseconds and tokens

## Blocking safety and quality metrics

Any of these block a supported claim:

```text
max_missed_critical_context_rate: 0.05
max_hallucinated_or_stale_context_rate: 0.0
max_regression_rate: 0.0
max_context_caused_regression_rate: 0.0
```

Stale or misleading context is a product failure, not a harmless retrieval miss. A run that succeeds despite stale context must still count against the stale-context gate.

## Negative-control rules

Negative controls test abstention. Layers should avoid injecting unnecessary context for tasks where repo/context assistance is not useful.

Required gates:

```text
min_negative_control_abstention_rate: 0.95
max_unnecessary_context_injection_rate: 0.05
```

For negative controls, unnecessary context injection includes any broad packet, unrelated file inspection caused by a packet, or generated context above the task spec's abstention threshold.

## Required artifacts

A benchmark run is not reviewable unless it saves local artifacts for every paired run:

- frozen task spec
- baseline transcript
- Layers transcript
- generated packet for Layers variants
- packet validation output
- packet inspection output where applicable
- validation command logs and exit codes
- JSONL run records
- final human and JSON benchmark reports

Required gates:

```text
require_confidence_intervals: true
require_raw_artifacts: true
require_preregistered_tasks: true
```

## Claim status semantics

- `supported`: all sample-size, primary-endpoint, secondary-endpoint, safety, negative-control, artifact, and uncertainty gates pass.
- `not_supported`: a hard blocker fails, such as success regression, stale-context rate above zero, regressions, missing artifacts, or failed negative-control abstention.
- `inconclusive`: there is no hard blocker, but sample size or uncertainty is insufficient.

## No-moving-goalposts rule

Do not relax thresholds, remove failed tasks, alter task prompts, or reclassify task categories after seeing results. Product fixes may be made after a failed run, but the next run must clearly use the same preregistered corpus or explain why a stricter corpus replaced it.

## Stronger future gate

After the first credible run, prefer a stricter version before making broader claims:

```text
min_paired_tasks: 50
min_success_delta: 0.05
min_token_reduction_ratio: 0.30
max_missed_critical_context_rate: 0.02
min_negative_control_abstention_rate: 0.98
```
