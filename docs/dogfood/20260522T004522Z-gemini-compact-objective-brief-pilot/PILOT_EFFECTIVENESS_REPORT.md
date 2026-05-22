# Gemini Compact Objective Brief Pilot

Status: scoped pilot evidence supported; full preregistered product claim remains not supported.

## Question

Can the current compact Objective Brief targeted-preflight workflow beat or at least match no-Layers baseline on real paired agent work while improving workflow/context quality?

## Run

- Artifact root: `docs/dogfood/20260522T004522Z-gemini-compact-objective-brief-pilot`
- Agent: `gemini-cli`
- Variants: `baseline`, `layers_targeted_preflight`
- Tasks: 5 paired tasks
  - Code-heavy pairs: 3
  - Negative-control pairs: 2
- Workflow records: 10
- Runner completion: 10/10 completed, 0 failed
- Packet Objective Brief artifacts: 3
- Full JSON packet validation artifacts: 3 generated, 3 valid
- Actionable secret findings: 0

## Headline metrics

| Metric | Baseline | Layers targeted-preflight | Delta |
|---|---:|---:|---:|
| Success rate | 1.000 | 1.000 | 0.000 |
| Median wall time | 380543 ms | 374633 ms | faster |
| Average wall time | 267647.6 ms | 254421.0 ms | 13226.6 ms saved/task |
| Speedup | — | 1.052x | positive |
| Average total tokens | 1846.2 | 2680.8 | -834.6 net estimated tokens |
| Token reduction ratio | — | -0.452 | blocker; estimated accounting |
| Context quality delta | — | +0.514 | positive |
| Missed critical context rate | 0.000 | 0.000 | no regression |
| Hallucinated/stale context rate | 0.000 | 0.000 | no regression |
| Negative-control abstention | — | 1.000 | pass |
| Unnecessary context injection | — | 0.000 | pass |

## Effectiveness verdict

Scoped pilot verdict: **supported for non-inferior success, faster average execution, improved context quality, and safe negative-control abstention on this 5-pair sample.**

Evidence:

1. The Layers targeted-preflight variant completed every paired task with the same success rate as baseline: `1.000` vs `1.000`.
2. It was faster on average by `13226.6 ms` per paired task, a `1.052x` speedup.
3. It improved context quality by `+0.514` with zero missed-critical-context, stale/hallucinated-context, or regression rate.
4. It abstained on negative controls: abstention rate `1.000`, unnecessary context injection `0.000`.
5. The compact Objective Brief artifacts used by the agent were small enough for handoff use:
   - `code-bugfix-mcp-client-error-redaction`: 429 words
   - `code-feature-workflow-benchmark-human-surfaces`: 389 words
   - `code-stale-trap-prefer-current-task-spec`: 415 words
6. Full JSON packets regenerated for the same targeted tasks validate successfully with `layers packet validate`.

## What is still not proven

The full preregistered product claim is still **not_supported**.

Blocking metrics from `compare/workflow-benchmark-report.json`:

- `token_reduction_ratio`
- `paired_task_count`
- `code_heavy_paired_task_count`
- `negative_control_paired_task_count`

Why:

1. Sample size is only 5 paired tasks; the preregistered gate requires 30 paired tasks, 20 code-heavy paired tasks, and 5 negative-control paired tasks.
2. Estimated token accounting improved dramatically versus the earlier full-packet run but is still net negative in this pilot:
   - Earlier Phase 15 targeted preflight overhead: `4766.0` estimated tokens
   - Current compact Objective Brief overhead: `551.2` estimated tokens
   - Improvement: about `88.4%` lower estimated overhead
   - Current estimated net token reduction ratio: `-0.452`
3. Therefore, this run supports a scoped workflow-effectiveness claim, not the stronger token-savings or full-product claim.
4. Token fields in this pilot are analyzer estimates (`measured_runs = 0`, `estimated_runs = 10`, `placeholder_runs = 0`), so they are useful for directionality and blocker detection but not a measured billing-token claim.
5. The full JSON packets regenerated for validation include expected local-workspace warnings such as dirty worktree / degraded impact metadata. They still pass `layers packet validate`; the Objective Briefs used by the agent are compact handoff artifacts, not full packet JSON.
6. `finalize-run.json` reports the unsupported benchmark claim under `missing_required_artifacts`; in this run that field means claim-gate failure, not a missing file.

## Comparison to previous packet-heavy pilot

Previous committed Phase 15 report:

- Success delta: `0.000`
- Speedup: `1.161x`
- Context quality delta: `+0.514`
- Layers overhead: `4766.0` tokens
- Token reduction ratio: `-5.945`

Current compact Objective Brief pilot:

- Success delta: `0.000`
- Speedup: `1.052x`
- Context quality delta: `+0.514`
- Layers overhead: `551.2` estimated tokens
- Token reduction ratio: `-0.452` estimated

Conclusion: compact Objective Brief injection preserved the success/context-quality signal while reducing estimated Layers overhead by about `88.4%`. It is still not yet token-positive.

## Artifact map

- Runner plan: `runner-plan.json`
- Execution report: `runner-execution.stdout.json`
- Workflow records: `compare/workflow-runs.jsonl`
- Analyzer JSON: `compare/workflow-benchmark-report.json`
- Analyzer Markdown: `compare/workflow-benchmark-report.md`
- Finalizer JSON: `finalize-run.json`
- Independent scoring: `independent-scoring.json`
- Secret scan: `SECRET_SCAN.md`
- Objective Briefs used by targeted-preflight agent runs: `packets/*.md`
- Full JSON packet validation proof: `packets/json/*.json` and `packets/json-validation/*.validate.exit`
- Transcripts: `transcripts/*.md`
- Validation logs: `validation/*.log`
- Diffs: `diffs/*.stat`, `diffs/*.patch`

## Next blocker to remove

The next product slice should attack net token reduction without sacrificing the positive pilot signals:

1. Cut the fixed benchmark prompt boilerplate and duplicate task text around Objective Brief injection.
2. Keep negative-control abstention at 100%.
3. Reduce code-task Objective Brief overhead below ~250-300 tokens if possible.
4. Rerun at least a 10-task calibration, then the 30-task preregistered proof only after token ratio is non-negative in calibration.
