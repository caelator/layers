# Layers Workflow Benchmarks

This directory contains the preregistered task specs, claim gates, schemas, fixtures, and run protocol for comparing Layers-assisted workflows against no-Layers baselines.

The first proof target is intentionally narrow: targeted Layers preflight for code-heavy repo tasks. Broad `layers query` and MCP preflight are separate benchmark surfaces and must not be averaged into the targeted-preflight claim.

## Directory layout

```text
benchmarks/workflows/
  CLAIM_GATES.md
  README.md
  tasks/*.json
  fixtures/*.json
  schemas/*.json
  templates/*.json
  templates/*.md
```

Generated dogfood proof artifacts belong under `docs/dogfood/<timestamp>-layers-vs-baseline-proof/`, not in this benchmark definition directory.

## Task specs

Each task spec is fixed before a proof run and should include:

- `task_id`
- `title`
- `prompt`
- `category`
- `difficulty`
- `surface_claim`
- `negative_control`
- `stale_context_trap`
- `target_files`
- `expected_relevant_files`
- `expected_validation_commands`
- `success_rubric`
- `abstention_rubric` for negative controls

Code-heavy tasks must name expected relevant files and validation commands. Negative controls must define what unnecessary context injection means.

## Benchmark variants

Use distinct variants:

- `baseline`: no Layers packet.
- `layers_targeted_preflight`: targeted `layers preflight --no-audit --json --strict --target ...`.
- `layers_broad_query`: broad `layers query --json ...`; report separately.
- `layers_mcp_preflight`: MCP stable preflight surface; report separately.

Legacy `layers` records, if accepted for old dogfood artifacts, must not silently mix broad query and targeted preflight in new proof reports.

## Run records

Workflow runs are recorded as JSONL and analyzed with:

```sh
cargo run -q -- workflow-benchmark analyze docs/dogfood/<run>/compare/workflow-runs.jsonl
cargo run -q -- workflow-benchmark analyze docs/dogfood/<run>/compare/workflow-runs.jsonl --json
```

Every task needs paired baseline and Layers records for the claim variant. The same agent/model/tool permissions/time budget should be used for both variants, with order randomized or otherwise documented.

## Claim status

Reports must use one of three statuses:

- `supported`: all preregistered gates pass.
- `not_supported`: a hard blocker fails, such as lower verified success, stale context, regressions, failed negative controls, or missing artifacts.
- `inconclusive`: sample size or uncertainty is insufficient but no hard blocker has failed.

Do not state “definitive proof” unless the exact scoped claim passed the gates in `CLAIM_GATES.md` and raw artifacts are reviewable.

## Validation

After the validation command is implemented, validate the corpus with:

```sh
cargo run -q -- workflow-benchmark validate-tasks benchmarks/workflows/tasks
cargo run -q -- workflow-benchmark validate-tasks benchmarks/workflows/tasks --json
```

A valid corpus must include normal code-heavy tasks and negative controls. The first credible proof run requires at least 30 paired tasks including at least 20 code-heavy tasks and 5 negative controls.
