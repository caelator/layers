# Phase 15 Fixed Workflow Benchmark Final Report

- Artifact root: `/Users/xxx/layers/docs/dogfood/20260513T2355Z-phase15-fixed-validpacket-minibatch`
- Workflow records: 10
- Expected/completed/failed runs: 10 / 10 / 0
- Packet artifacts: 3
- Packet validation failures: 0
- Missing required artifacts: 1
- Secret-shaped artifact findings: 0
- Claim status: NotSupported
- Blocking metrics: token_reduction_ratio, average_layers_overhead_tokens, paired_task_count, code_heavy_paired_task_count, negative_control_paired_task_count
- Uncertainty notes: paired_task_count 5 is below minimum 30; code_heavy_paired_task_count 3 is below minimum 20; negative_control_paired_task_count 2 is below minimum 5

## Paired comparison
- Paired tasks: 5
- Speedup: 1.161x
- Net time saved per task: 34595.8 ms
- Token reduction ratio: -5.945
- Success delta: 0.000
- Context quality delta: 0.514

## Blocking artifact findings
- benchmark claim not supported: token_reduction_ratio, average_layers_overhead_tokens, paired_task_count, code_heavy_paired_task_count, negative_control_paired_task_count
