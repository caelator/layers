# Phase 5 Failure Analysis

Source artifacts:
- `compare/workflow-benchmark-report.json`
- `compare/workflow-benchmark-report.md`
- `compare/workflow-runs.jsonl`

## Claim status

Status: `not_supported`

Blocking metrics:
- `success_delta`
- `token_reduction_ratio`

Summary:
- Paired tasks: 30
- Baseline success rate: 0.133
- Layers success rate: 0.100
- Success delta: -0.033
- Token reduction ratio: -3.567
- Average Layers overhead: 0.8 ms / 1890.2 tokens

## Regressed pairs

Layers regressed three paired tasks relative to the no-Layers baseline:

| Task | Category | Baseline | Layers | Layers overhead tokens | Baseline input tokens | Layers input tokens |
|------|----------|----------|--------|------------------------|-----------------------|---------------------|
| `code-feature-quality-abstain-low-specificity` | feature | 1.0 | 0.0 | 2647 | 245 | 2983 |
| `code-bugfix-mcp-client-error-redaction` | bugfix | 1.0 | 0.0 | 2474 | 232 | 2795 |
| `negative-control-simple-json-validity` | negative_control | 1.0 | 0.0 | 0 | 217 | 236 |

## Highest targeted-preflight overhead pairs

| Task | Category | Overhead tokens | Success delta |
|------|----------|-----------------|---------------|
| `code-bugfix-context-routing` | bugfix | 2714 | 0.0 |
| `code-refactor-context-compiler-packet-finalize` | refactor | 2696 | +1.0 |
| `code-feature-quality-abstain-low-specificity` | feature | 2647 | -1.0 |
| `code-feature-mcp-preflight-stable-registry` | feature | 2533 | 0.0 |
| `code-bugfix-provider-budget-overflow` | bugfix | 2522 | 0.0 |
| `code-bugfix-preflight-strict-low-relevance` | bugfix | 2490 | 0.0 |
| `code-bugfix-mcp-client-error-redaction` | bugfix | 2474 | -1.0 |

## Interpretation

The benchmark does not show product-effectiveness yet. The packet compiler is useful qualitatively, but the measured agent workflow claim is blocked by two facts:

1. The Layers variant underperformed the baseline on success rate.
2. The Layers variant added thousands of context tokens per run.

The immediate product priority is therefore not broader retrieval or more surfaces. It is smaller default injection artifacts and failure-mode fixtures for the regressed tasks.

## Follow-up fixtures

Keep these as regression fixtures until the benchmark claim turns supported:

1. `code-feature-quality-abstain-low-specificity`
   - Expected fix direction: abstain or provide a much smaller targeted brief when quality context is weak.
2. `code-bugfix-mcp-client-error-redaction`
   - Expected fix direction: preserve exact security/error-redaction constraints without large surrounding packet body.
3. `negative-control-simple-json-validity`
   - Expected fix direction: keep negative-control abstention and prevent any context-driven distraction.
4. Any targeted-preflight packet above 2,000 overhead tokens
   - Expected fix direction: inject objective brief or graded compact summary by default; keep full packet as a side artifact only.

## Current guardrail

`src/cmd/workflow_benchmark.rs` includes a committed Phase 5 claim fixture test that keeps this report classified as `not_supported` while `success_delta` and `token_reduction_ratio` remain blockers.
