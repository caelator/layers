# Phase 12 runner execution smoke

## Scope

This phase adds and dogfoods a real `workflow-benchmark run-plan` execution layer for runner plans produced by Phase 11. It is intentionally a smoke/infrastructure slice, not product-effectiveness evidence.

The smoke run verifies that the runner can:

- read a deterministic `runner-plan.json`
- create isolated git worktrees for paired variants
- keep the no-Layers baseline isolated from preflight execution
- run a targeted-preflight command only for `layers_targeted_preflight`
- invoke a smoke-friendly agent command with exact prompt stdin
- write transcripts, validation logs, packet artifacts, JSONL run records, and an execution report
- remove worktrees by default after execution

## Artifacts

Artifact directory:

`docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke/`

Key files:

- `tasks/phase12-smoke-task.json` — one validated synthetic smoke task
- `scripts/smoke-agent.py` — deterministic smoke agent, writes `agent-output.txt` in each worktree
- `scripts/smoke-preflight.py` — deterministic targeted-preflight packet producer
- `runner-plan.json` — paired baseline vs `layers_targeted_preflight` plan
- `execution-order.jsonl` — deterministic run order
- `prompts/*.md` — exact prompts captured before execution
- `transcripts/*.md` — per-run execution transcript
- `validation/*.log` — per-run validation command log
- `packets/*.json` — targeted-preflight packet artifact
- `compare/workflow-runs.jsonl` — analyzer-compatible smoke run records
- `compare/runner-execution-report.json` — execution summary
- `compare/analyzer-report.json` — analyzer output over the smoke run records

## Commands run

Task validation:

```sh
cargo run -q -- workflow-benchmark validate-tasks \
  docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke/tasks
```

Plan generation:

```sh
cargo run -q -- workflow-benchmark plan-run \
  docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke/tasks \
  --output-dir docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke \
  --repo-root /Users/xxx/layers \
  --agent-command 'python3 /Users/xxx/layers/docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke/scripts/smoke-agent.py' \
  --model phase12-smoke \
  --seed 12 \
  --json
```

Execution smoke:

```sh
cargo run -q -- workflow-benchmark run-plan \
  docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke/runner-plan.json \
  --preflight-command 'python3 /Users/xxx/layers/docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke/scripts/smoke-preflight.py' \
  --json
```

Analyzer smoke:

```sh
cargo run -q -- workflow-benchmark analyze \
  docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke/compare/workflow-runs.jsonl \
  --json > docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/phase12-runner-execution-smoke/compare/analyzer-report.json
```

## Results

Runner execution report:

- total runs: 2
- completed runs: 2
- failed runs: 0
- variants: `baseline`, `layers_targeted_preflight`
- generated run records: 2

Analyzer smoke report:

- claim status: `not_supported`
- expected uncertainty notes include insufficient sample size:
  - paired task count below 30
  - code-heavy paired task count below 20
  - negative-control paired task count below 5

## Claim boundary

This phase proves only that isolated runner execution plumbing works on a synthetic smoke task. It does not execute the preregistered paired coding-agent corpus and does not support a Layers effectiveness claim.

Any future product claim still requires real independent coding-agent runs over the preregistered corpus with the Phase 10/`CLAIM_GATES.md` gates, including at least 30 paired tasks, 20 code-heavy paired tasks, and 5 negative-control paired tasks.
