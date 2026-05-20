# Phase 15 Fixed Workflow Benchmark Final Report

- Artifact root: `/Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration`
- Workflow records: 20
- Expected/completed/failed runs: 20 / 20 / 0
- Packet artifacts: 8
- Packet validation failures: 0
- Missing required artifacts: 1
- Secret-shaped artifact findings: 0
- Claim status: NotSupported
- Blocking metrics: net_time_saved_ms, token_reduction_ratio, paired_task_count, code_heavy_paired_task_count, negative_control_paired_task_count
- Uncertainty notes: paired_task_count 10 is below minimum 30; code_heavy_paired_task_count 8 is below minimum 20; negative_control_paired_task_count 2 is below minimum 5

## Paired comparison
- Paired tasks: 10
- Speedup: 0.887x
- Net time saved per task: -48434.4 ms
- Token reduction ratio: -1.361
- Success delta: 0.000
- Context quality delta: 0.686

## Blocking artifact findings
- benchmark claim not supported: net_time_saved_ms, token_reduction_ratio, paired_task_count, code_heavy_paired_task_count, negative_control_paired_task_count
