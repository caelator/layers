# Compact Objective Brief Benchmark Rerun

Status: partial rerun / claim gate remains **not supported**.

## What was rerun

A fresh paired runner plan was generated from the Phase 5 preregistered 30-task benchmark using the current default targeted-preflight command:

```bash
layers preflight --no-audit --agent-prompt --strict
```

Artifacts in this directory:

- `runner-plan.json` — fresh 30-task paired benchmark plan.
- `execution-order.jsonl` — deterministic execution order.
- `prompts/` — 60 baseline/layers prompts from the fresh plan.
- `compact-preflight-accounting.json` — compact Objective Brief accounting over the 24 non-negative-control tasks.
- `compact-packets/` — generated compact Objective Brief artifacts for the non-negative-control tasks.

## Live runner status

The full live Codex runner was started, but the first baseline run stayed active for more than 17 minutes without producing validation/diff artifacts. The run was killed rather than letting a wedged agent process block the rest of the product slice.

This means there is no new completed paired agent-success report from this attempt.

## Token / prompt overhead result

Compared with the committed Phase 5 full JSON packet artifacts, the current compact Objective Brief artifacts are materially smaller:

| Metric | Phase 5 full JSON packets | Current compact Objective Brief |
|--------|---------------------------|---------------------------------|
| Non-negative-control tasks | 24 | 24 |
| Average artifact words | 1041.9 | 421.5 |
| Max artifact words | 1174 | 484 |
| Artifact word reduction | — | 59.5% |

Interpretation: the compact Objective Brief default fixed the most obvious packet-size regression in the artifact itself.

## Product claim gate

Still **not supported**.

Reasons:

1. The live paired agent benchmark did not complete, so no new success delta exists.
2. The latest completed 30-pair benchmark remains the Phase 5 report:
   - baseline success: 0.133
   - Layers success: 0.100
   - success delta: -0.033
   - claim status: not_supported
3. The compact artifact-size improvement is encouraging, but it does not prove Layers is better than no context.

## Current conclusion

Supported:

- Current targeted preflight now defaults to compact Objective Brief output.
- Compact artifacts are roughly 60% smaller than prior full JSON packet artifacts for the same preregistered non-negative-control tasks.
- Token/word overhead is now plausibly in the right direction.

Not yet supported:

- Layers improves coding-agent success rate over baseline.
- Layers is better than no context across paired real-agent runs.

Next benchmark action:

- Re-run the paired benchmark with a bounded agent-command wrapper or a smaller preregistered smoke subset that enforces per-run timeout and produces validation/diff artifacts, then only claim improvement if `success_delta >= 0` and negative-control distraction stays low.
