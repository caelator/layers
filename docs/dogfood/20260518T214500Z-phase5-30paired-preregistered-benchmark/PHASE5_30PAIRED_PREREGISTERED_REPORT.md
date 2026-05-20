# Phase 5 30-paired preregistered benchmark report

Status: completed with blocking failure; not usable as product-effectiveness proof.

Run: `/Users/xxx/layers/docs/dogfood/20260518T214500Z-phase5-30paired-preregistered-benchmark`

## Runner outcome
- workflow_records: 60
- paired_tasks: 30
- runner_total_runs: 60
- runner_completed_runs: 7
- runner_failed_runs: 53
- nonzero_agent_exits: 53
- nonzero_validation_exits: 0
- transcripts: 60
- validation_logs: 60
- diff_stats: 60
- diff_patches: 60
- nonempty_patches: 6
- packet_json: 24
- packet_validation_failures: 0
- live_secret_findings: 0
- synthetic_secret_matches: 0

## Dominant failure mode
Gemini CLI quota exhaustion (429 QUOTA_EXHAUSTED) for 53/60 agent runs

The runner process itself exited 0 and wrote final artifacts, but the run records show most agent subprocesses exited 1. Transcript tails identify Gemini quota exhaustion with HTTP 429 / QUOTA_EXHAUSTED. This is an infrastructure/model-capacity failure, not evidence that Layers hurt or helped task performance.

## Claim status
- claim_status: not_supported
- product_effectiveness_evidence_usable: false

## Secret scan
- live findings: 0
- synthetic/test-fixture matches: 0

## Next action
Do not treat this Phase5 run as the preregistered proof. Preserve it as a failed infrastructure/capacity run, then rerun Phase5 after switching to a model/provider budget that can complete 60 real-agent runs or after reducing concurrency/scope with a fresh preregistered run directory.
