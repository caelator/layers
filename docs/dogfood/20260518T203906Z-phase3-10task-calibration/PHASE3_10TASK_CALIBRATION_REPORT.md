# Phase3 10-task calibration report

Run directory: `/Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration`

## Artifact health

- Planned/runner total runs: 20
- Completed runs: 20
- Failed runs: 0
- Workflow records: 20
- Paired tasks: 10
- Variants: {'baseline': 10, 'layers_targeted_preflight': 10}
- Agent nonzero exits: 0
- Validation nonzero exits: 0
- Transcripts: 20
- Validation logs: 20
- Diff stats/patches: 20 / 20
- Non-empty patches: 16
- Empty patches: 4 (expected for the two negative-control paired no-op tasks)
- Packet JSON artifacts: 8
- Packet validation failures: 0
- Targeted negative-control abstentions: 2
- Unnecessary context injections: 0
- Live secret-shaped findings: 0
- Synthetic/test-fixture secret-shaped matches: 2

## Claim status

This is calibration/training-data collection, not product-effectiveness proof.

- Analyzer claim status: `not_supported`
- Finalizer success flag: `None`
- Finalizer claim status: `not_supported`
- Blocking findings: `['benchmark claim not supported: net_time_saved_ms, token_reduction_ratio, paired_task_count, code_heavy_paired_task_count, negative_control_paired_task_count']`

## Training-data quality

The run is useful as calibration/eval seed data because all 20 real-agent runs completed with validation logs, transcripts, and preserved diffs. It is not sufficient for the preregistered product claim because it has only 10 paired tasks, below the 30 paired / 20 code-heavy / 5 negative-control gates.

## Next

Phase4 should audit/expand the corpus and select a 30-paired preregistered run set that satisfies the frozen sample gates before Phase5.
