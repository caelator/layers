Workflow benchmark report
=========================
Runs: 10
Paired tasks: 5

Baseline (no Layers)
- Runs: 5
- Success rate: 1.000
- Median wall time: 380543.0 ms
- Average total tokens: 1846.2
- Context relevant/waste ratios: 0.000 / 0.000
- Verification/change/planning quality: 1.00 / 1.00 / 1.00

Layers targeted-preflight
- Runs: 5
- Success rate: 1.000
- Median wall time: 374633.0 ms
- Average total tokens: 2680.8
- Context relevant/waste ratios: 0.919 / 0.000
- Verification/change/planning quality: 1.00 / 1.00 / 1.00

Paired net benefit vs. baseline: Layers targeted-preflight
- Paired tasks: 5
- Net time saved per task: 13226.6 ms
- Net tokens saved per task: -834.6
- Speedup: 1.052x
- Token reduction ratio: -0.452
- Success delta: 0.000
- Human intervention delta: 0.000
- Layers overhead: 0.6 ms / 551.2 tokens

Claim status: NotSupported
Blocking metrics: token_reduction_ratio, paired_task_count, code_heavy_paired_task_count, negative_control_paired_task_count
Uncertainty notes:
  - paired_task_count 5 is below minimum 30
  - code_heavy_paired_task_count 3 is below minimum 20
  - negative_control_paired_task_count 2 is below minimum 5
