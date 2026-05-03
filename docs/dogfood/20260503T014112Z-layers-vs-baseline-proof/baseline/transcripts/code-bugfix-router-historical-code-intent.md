# Workflow Benchmark Transcript

## Setup
- task_id: code-bugfix-router-historical-code-intent
- variant: baseline
- category: bugfix
- benchmark_run: 20260503T014112Z-layers-vs-baseline-proof
- repo_commit: e3e3763977238dc923e8c6c52ae25f3af76e6f09
- random_seed: phase8-first-full-local-artifact-set-v1

## Prompt
Fix routing so historical or prior-decision questions that mention code terms remain memory-eligible unless they clearly request edit/debug/action work. Add benchmark-style routing tests.

## Packet Artifacts
- none

## Timeline
- This Phase 8 artifact execution is a local protocol/preflight pass, not an independent coding-agent task-solving run.
- Code-heavy tasks were not edited; their success is scored 0.0 to avoid overstating evidence.
- Negative controls were answered/validated directly and targeted-preflight was treated as abstaining from Layers context.

## Tool and Command Log
- baseline/validation/code-bugfix-router-historical-code-intent.not-executed.log

## Changes Made
None in task worktrees for this first artifact set.

## Validation
status: not_executed

## Scoring Notes
Code-heavy baseline implementation was not executed; scored 0.0 to keep claim boundaries honest.

## Context Quality Classification
- missed_critical_context: 1
- hallucinated_or_stale_context: 0
- unnecessary_context_injections: 0
- negative_control_abstained: false
