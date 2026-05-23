Workflow benchmark report
=========================
Runs: 10
Paired tasks: 5

Baseline (no Layers)
- Runs: 5
- Success rate: 1.000
- Median wall time: 334894.0 ms
- Average total tokens: 1570.4
- Context relevant/waste ratios: 0.000 / 0.000
- Verification/change/planning quality: 1.00 / 1.00 / 1.00

Layers targeted-preflight
- Runs: 5
- Success rate: 1.000
- Median wall time: 309925.0 ms
- Average total tokens: 10905.8
- Context relevant/waste ratios: 1.000 / 0.000
- Verification/change/planning quality: 1.00 / 1.00 / 1.00

Paired net benefit vs. baseline: Layers targeted-preflight
- Paired tasks: 5
- Net time saved per task: 34595.8 ms
- Net tokens saved per task: -9335.4
- Speedup: 1.161x
- Token reduction ratio: -5.945
- Success delta: 0.000
- Human intervention delta: 0.000
- Layers overhead: 0.6 ms / 4766.0 tokens

Claim status: NotSupported
Blocking metrics: token_reduction_ratio, average_layers_overhead_tokens, paired_task_count, code_heavy_paired_task_count, negative_control_paired_task_count
Uncertainty notes:
  - paired_task_count 5 is below minimum 30
  - code_heavy_paired_task_count 3 is below minimum 20
  - negative_control_paired_task_count 2 is below minimum 5
