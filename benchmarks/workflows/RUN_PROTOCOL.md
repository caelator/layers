# Layers Workflow Benchmark Run Protocol

This protocol defines reproducible manual or agent-assisted runs for the preregistered Layers-vs-baseline workflow benchmark. It is intentionally operational: each run must leave enough local artifacts for another reviewer to reconstruct the prompt, context surface, changes, validation, timing, and scoring decision.

## Scope

Primary comparison:
- `baseline`: agent receives only the task spec and repo access.
- `layers_targeted_preflight`: agent must first generate and inspect a targeted Layers preflight packet, then use only relevant packet content.

Keep `layers_broad_query` and `layers_mcp_preflight` as separate variants if they are run later. Do not aggregate them into the targeted-preflight claim.

## Checkout/reset procedure

Use an isolated worktree per task/variant when possible:

```sh
git -C /Users/xxx/layers worktree add /tmp/layers-bench-<task_id>-<variant> HEAD
cd /tmp/layers-bench-<task_id>-<variant>
```

If a worktree is not practical, record why in the transcript and reset from `/Users/xxx/layers` before each variant:

```sh
git status --short
git checkout -- .
git clean -fd
git status --short
```

Never delete or mutate the benchmark artifact directory during reset. Prefer storing artifacts outside the worktree under `docs/dogfood/<timestamp>-layers-vs-baseline-proof/`.

## Agent/model used

Record the exact agent, model, provider, and invocation surface in the transcript and run record, for example:

```text
agent: hermes-cli
model: gpt-5.5
provider: openai-codex
```

The same agent/model/provider should be used for paired baseline and Layers variants unless the run is explicitly marked invalid for primary claims.

## Tool permissions

Use the same tool permissions for paired variants:
- file read/write inside the checked-out repo and artifact directory
- terminal commands needed for build/test/validation
- git status/diff/log commands
- no production deployments
- no external network unless the task spec explicitly requires it

Additional Layers restriction for baseline:
- do not run `layers query`, `layers preflight`, `layers mcp`, or consume previously generated Layers packets.

Additional Layers requirement for targeted preflight:
- generate the packet before implementation.
- validate and inspect the packet before using it.
- record weak, stale, misleading, or low-confidence packet behavior instead of silently trusting it.

## Time budget

Default per-variant budget: 30 minutes unless the task spec sets `time_budget_minutes`.

Record:
- `wall_time_ms`
- `orientation_ms`
- `implementation_ms`
- `debugging_ms`
- `verification_ms`

The phase times must not exceed wall time. If exact timing is unavailable, use conservative stopwatch estimates and mark the transcript as estimated.

## Randomized order

Randomize paired variant order before starting a task to reduce learning/order effects. Use a deterministic seed recorded in the artifact set:

```sh
python3 - <<'PY'
import random
seed = '<run_id>:<task_id>'
variants = ['baseline', 'layers_targeted_preflight']
r = random.Random(seed)
r.shuffle(variants)
print(seed)
print('\n'.join(variants))
PY
```

Record the seed and resulting order in the transcript. If order is not randomized, mark the run invalid for primary claims unless there is a documented reason.

## Baseline prompt format

```text
You are evaluating a benchmark task without Layers context.
Task ID: <task_id>
Task: <prompt>
Allowed repo: /Users/xxx/layers
Expected relevant files: <expected_relevant_files>
Expected validation commands: <expected_validation_commands>
Rules:
- Do not run `layers query`, `layers preflight`, `layers mcp`, or use generated Layers packets.
- Complete the task or explain why it cannot be completed.
- Run the expected validation commands if you change code.
- Save transcript notes to <artifact_dir>/transcripts/baseline/<task_id>.txt.
```

## Targeted preflight prompt format

```text
You are evaluating a benchmark task with targeted Layers preflight.
Task ID: <task_id>
Task: <prompt>
Allowed repo: /Users/xxx/layers
Expected relevant files: <expected_relevant_files>
Expected validation commands: <expected_validation_commands>
Before implementation, run:
  cargo run -q -- preflight --no-audit --json --strict <targets> "<prompt>" > <packet_path>
Then validate and inspect the packet:
  cargo run -q -- packet validate <packet_path> > <packet_validate_log> 2>&1
  cargo run -q -- packet inspect --json <packet_path> > <inspect_path>
Use only relevant packet content. If the packet is weak/stale/misleading, record that and proceed carefully.
Run expected validation commands if you change code.
Save transcript notes to <artifact_dir>/transcripts/layers-targeted-preflight/<task_id>.txt.
```

Use task-spec `target_files` to construct `<targets>` as repeated `--target <path>` arguments. If a non-negative-control task has no target files, record a protocol violation.

## Saving packet artifacts

For targeted-preflight runs, save:

```text
<artifact_dir>/packets/targeted-preflight/<task_id>.json
<artifact_dir>/packets/targeted-preflight/<task_id>.validate.log
<artifact_dir>/packets/targeted-preflight/<task_id>.validate.exit
<artifact_dir>/packets/targeted-preflight/<task_id>.inspect.json
```

Also record packet route, confidence, section ids, warnings, budget used, and validation exit status in the transcript. Packet generation time and token/context overhead should be included in Layers overhead fields.

## Scoring success

Score only after validation evidence is collected.

Use the task spec `success_rubric`:
- `success_score = 1.0`: full success, expected validation passes, no material regressions.
- `success_score = 0.5`: partial success, useful progress but incomplete validation or minor correctness gap.
- `success_score = 0.0`: failure, no usable fix/output, wrong task, validation failure, or context-caused regression.

Quality scores use `0..=5`:
- `verification_quality`
- `change_quality`
- `planning_quality`
- `reproducibility`
- `confidence_calibration`
- `user_usefulness`

Record explicit rationale for every score below 5.

## Tokens, tool calls, and time

Record:
- `input_tokens`, `output_tokens`, `peak_context_tokens`
- `context_relevant_tokens`, `context_duplicate_tokens`, `context_irrelevant_tokens`
- `assistant_turns`, `tool_calls`, `failed_commands`, `patch_attempts`, `test_runs`
- `human_interventions`, `failed_attempts`
- `layers_overhead_ms`, `layers_overhead_tokens`

If the agent runtime does not expose token counts, estimate from transcript text and mark the counts as estimates in transcript notes. Do not leave required JSON fields blank; use `0` only when genuinely zero or unavailable with an explicit transcript note.

Baseline records must report zero Layers overhead.

## Missed critical context

`missed_critical_context` counts task-critical information that was available from expected relevant files, task spec, packet, or validation logs but was not used and materially hurt the result.

Examples:
- ignored expected target file
- missed existing helper/API and duplicated incompatible logic
- failed validation because a cited constraint was missed

Do not count harmless unused context as missed critical context.

## Stale context

`hallucinated_or_stale_context` counts stale, fabricated, or obsolete context that materially influenced the run.

Examples:
- used memory/packet text that contradicted current code
- cited a file/symbol that no longer exists
- trusted a deprecated runtime surface for a stable-core task

If stale context is present but ignored after being identified, record it in transcript notes but do not increment the metric unless it affected behavior.

## Unnecessary context injection

For negative controls, `unnecessary_context_injections` counts cases where a Layers variant injected or used repo/project context when the task should have abstained or needed no context.

A negative-control Layers run should normally set:
- `negative_control_abstained: true`
- `unnecessary_context_injections: 0`
- `context_caused_regressions: 0`

If injected context slows, confuses, or changes an otherwise trivial answer, count it and explain why.

## Run record and transcript artifacts

For each variant, create:

```text
<artifact_dir>/transcripts/<variant>/<task_id>.txt
<artifact_dir>/validation/<variant>/<task_id>.<command>.log
<artifact_dir>/validation/<variant>/<task_id>.<command>.exit
```

Append one JSON line based on `benchmarks/workflows/templates/workflow-run-record.json` to:

```text
<artifact_dir>/compare/workflow-runs.jsonl
```

Then verify the JSONL with:

```sh
cargo run -q -- workflow-benchmark analyze <artifact_dir>/compare/workflow-runs.jsonl --json > <artifact_dir>/compare/workflow-benchmark-report.json
```

## Validity checks before claiming results

A run pair is valid for the primary claim only if:
- both variants used the same task spec, repo commit, model, tool permissions, and time budget
- order was randomized or the deviation is documented and excluded from primary claims
- baseline did not use Layers-generated context
- targeted preflight generated, validated, and inspected a packet before implementation
- transcripts, validation logs, packet artifacts, and JSONL records are present
- scoring references the task rubric and validation evidence

Do not change `benchmarks/workflows/CLAIM_GATES.md` after inspecting results unless making gates stricter and clearly marking the run as post-hoc.