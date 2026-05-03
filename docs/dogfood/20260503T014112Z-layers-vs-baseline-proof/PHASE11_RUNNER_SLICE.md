# Phase 11 Runner Planning Slice

Phase 11 adds the first automated isolated coding-agent benchmark runner slice. This is a planning/dry-run layer only: it creates deterministic paired run artifacts for future executor work, but it does not yet launch agents, create git worktrees, edit code, score outcomes, or support an effectiveness claim.

## Implemented scope

- Added `layers workflow-benchmark plan-run`.
- Added a `RunnerPlanConfig` and serializable runner plan model.
- Generates paired variants for each task spec:
  - `baseline`
  - `layers_targeted_preflight`
- Writes deterministic execution ordering from a supplied seed.
- Writes these reproducibility artifacts under the selected output directory:
  - `runner-plan.json`
  - `execution-order.jsonl`
  - `prompts/*.md`
  - `transcripts/*.md`
  - `packets/` directory for targeted-preflight packet outputs
  - `validation/` directory for future validation logs
  - `worktrees/` directory naming the isolated worktree paths for future execution
- Baseline prompts explicitly prohibit Layers commands and preflight artifacts.
- Targeted-preflight prompts instruct the future executor to run `layers preflight --no-audit --json --strict` and save the packet artifact before edits.

## Dogfood artifact

A small dry-run fixture artifact was generated here:

`docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase11-runner-plan/`

This fixture uses `benchmarks/workflows/fixtures/valid-task-spec.json` and produces exactly two planned runs, one per variant. It validates runner-planning plumbing without executing benchmark tasks.

## Claim boundary

This phase does not change the Phase 8/9/10 benchmark verdict. The current artifact set remains `not_supported` for product-performance/effectiveness claims because independent code-heavy coding-agent task execution has still not been run.

The next phase must add real isolated execution:

1. Create/reset throwaway worktrees from `runner-plan.json`.
2. Launch the same coding agent/model/tool budget per variant.
3. Capture full transcripts, packet validation/inspection outputs, validation logs, timings, token accounting, and run records.
4. Score task success and context quality.
5. Rerun analyzer and repeat independent review before making any targeted-preflight claim.
