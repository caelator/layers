# Workflow Benchmark Transcript

## Setup
- task_id: code-docs-architecture-context-spine
- variant: layers_targeted_preflight
- category: other
- benchmark_run: 20260503T014112Z-layers-vs-baseline-proof
- repo_commit: e3e3763977238dc923e8c6c52ae25f3af76e6f09
- random_seed: phase8-first-full-local-artifact-set-v1

## Prompt
Improve architecture documentation or CLI help to describe Layers as a local-first context compiler/context spine for coding agents, not a competing agent runtime. Keep claims evidence-gated.

## Packet Artifacts
- layers_targeted_preflight/packets/code-docs-architecture-context-spine.preflight.json
- layers_targeted_preflight/packets/code-docs-architecture-context-spine.preflight.stderr
- layers_targeted_preflight/packets/code-docs-architecture-context-spine.preflight.exit

## Timeline
- This Phase 8 artifact execution is a local protocol/preflight pass, not an independent coding-agent task-solving run.
- Code-heavy tasks were not edited; their success is scored 0.0 to avoid overstating evidence.
- Negative controls were answered/validated directly and targeted-preflight was treated as abstaining from Layers context.

## Tool and Command Log
- layers_targeted_preflight/packet-validate/code-docs-architecture-context-spine.json-parse.log
- layers_targeted_preflight/packet-inspect/code-docs-architecture-context-spine.summary.log
- layers_targeted_preflight/validation/code-docs-architecture-context-spine.not-executed.log

## Changes Made
None in task worktrees for this first artifact set.

## Validation
status: not_executed

## Scoring Notes
Targeted preflight artifact was generated, but code-heavy implementation was not executed; scored 0.0 to keep claim boundaries honest.

## Context Quality Classification
- missed_critical_context: 1
- hallucinated_or_stale_context: 0
- unnecessary_context_injections: 0
- negative_control_abstained: false
