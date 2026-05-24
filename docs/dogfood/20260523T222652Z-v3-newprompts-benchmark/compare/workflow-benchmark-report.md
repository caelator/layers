Workflow benchmark report
=========================
Runs: 62
Paired tasks: 31

Baseline (no Layers)
- Runs: 31
- Success rate: 0.000
- Median wall time: 24189.0 ms
- Average total tokens: 1143.9
- Context relevant/waste ratios: 0.000 / 0.000
- Verification/change/planning quality: 0.00 / 0.00 / 1.00

Layers targeted-preflight
- Runs: 31
- Success rate: 0.000
- Median wall time: 24115.0 ms
- Average total tokens: 2770.4
- Context relevant/waste ratios: 0.922 / 0.000
- Verification/change/planning quality: 0.00 / 0.00 / 1.00

Paired net benefit vs. baseline: Layers targeted-preflight
- Paired tasks: 31
- Net time saved per task: -262.4 ms
- Net tokens saved per task: -1626.4
- Speedup: 0.989x
- Token reduction ratio: -1.422
- Success delta: 0.000
- Human intervention delta: 0.000
- Layers overhead: 0.8 ms / 751.5 tokens

Claim status: NotSupported
Blocking metrics: net_time_saved_ms, token_reduction_ratio
