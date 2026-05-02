# Definitive Layers Proof Benchmark Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Build a preregistered, local-first, reproducible paired benchmark corpus and reporting workflow that can honestly decide whether targeted Layers context improves verified coding-agent outcomes over no Layers.

**Architecture:** Treat proof as a product/evidence system, not a one-off dogfood run. The repo already has `src/cmd/workflow_benchmark.rs`, claim gates, two example task specs under `benchmarks/workflows/tasks/`, and JSONL analysis. This plan hardens that system with preregistered claim gates, richer task specs, corpus validation, confidence intervals, per-surface comparisons, reproducible artifact layout, and enough benchmark tasks to make a skeptical engineer trust the result.

**Tech Stack:** Rust CLI (`layers workflow-benchmark`), Serde JSON/JSONL, local Markdown docs, shell validation gates, existing Layers packet validation/inspection commands, optional Python only for helper scripts/static artifact scans where Rust implementation is not yet warranted.

---

## Non-negotiable proof standard

The final benchmark may only claim support when all of these are true:

1. The claim was preregistered before the run.
2. The task corpus was fixed before the run.
3. Every task has paired baseline and Layers runs.
4. The same agent/model/tool permissions/time budget are used for both variants.
5. Success is the primary endpoint; token/time savings are secondary.
6. Layers overhead is included in Layers cost.
7. Negative controls are present and Layers abstains on them.
8. Stale/misleading context failures block broad claims.
9. Raw tasks, transcripts, packets, validation logs, and run records are committed or otherwise locally reviewable.
10. The report says `supported`, `not_supported`, or `inconclusive` without marketing language.

Initial proof target, intentionally narrow:

> On code-heavy repo tasks, targeted Layers preflight maintains or improves verified success compared with no Layers while reducing context/time cost after overhead and avoiding stale or unnecessary context injection.

Do not initially try to prove broad “Layers is better for everything.” Prove targeted preflight first.

---

## Existing repo state to preserve

Relevant files already exist:

- `src/cmd/workflow_benchmark.rs`
  - `WorkflowRun`
  - `ClaimThresholds`
  - claim status/reporting
  - JSONL loading and analysis
  - negative-control fields
- `benchmarks/workflows/tasks/code-bugfix-context-routing.json`
- `benchmarks/workflows/tasks/negative-control-trivial-arithmetic.json`
- `docs/dogfood/20260429T192051Z-layers-self-diagnostic/compare/workflow-runs.jsonl`
- `docs/dogfood/20260429T192051Z-layers-self-diagnostic/compare/workflow-benchmark-report.pretty.json`

Known current evidence problem:

- only 2 paired tasks / 4 runs
- current claim status is `not_supported`
- success delta was negative
- broad query and targeted preflight are not separated enough for a clean claim

---

## Target artifact layout

Create or maintain this structure:

```text
benchmarks/workflows/
  CLAIM_GATES.md
  README.md
  tasks/
    code-bugfix-context-routing.json
    negative-control-trivial-arithmetic.json
    ... 30+ total task specs ...
  schemas/
    task-spec.schema.json
    workflow-run.schema.json
  fixtures/
    valid-task-spec.json
    invalid-task-spec-missing-rubric.json

docs/dogfood/<timestamp>-layers-vs-baseline-proof/
  CLAIM.md
  TASKS.md
  README.md
  tasks/*.json
  packets/broad-query/*.json
  packets/targeted-preflight/*.json
  transcripts/baseline/*.txt
  transcripts/layers-targeted-preflight/*.txt
  transcripts/layers-broad-query/*.txt
  validation/*.log
  validation/*.exit
  compare/workflow-runs.jsonl
  compare/workflow-benchmark-report.json
  compare/workflow-benchmark-report.md
  compare/blocking-metrics.md
```

---

## Claim gates v1

Put these in `benchmarks/workflows/CLAIM_GATES.md` and mirror enforceable thresholds in Rust.

Minimum gates for `supported`:

```text
primary_claim: targeted_preflight_beats_baseline_for_code_heavy_repo_tasks
min_paired_tasks: 30
min_code_heavy_paired_tasks: 20
min_negative_control_paired_tasks: 5
min_success_delta: 0.0
min_token_reduction_ratio: 0.20
min_time_saved_ms: 0.0
max_missed_critical_context_rate: 0.05
max_hallucinated_or_stale_context_rate: 0.0
max_regression_rate: 0.0
min_negative_control_abstention_rate: 0.95
max_context_caused_regression_rate: 0.0
max_unnecessary_context_injection_rate: 0.05
require_confidence_intervals: true
require_raw_artifacts: true
require_preregistered_tasks: true
```

Recommended stronger gate after the first pass:

```text
min_paired_tasks: 50
min_success_delta: 0.05
min_token_reduction_ratio: 0.30
max_missed_critical_context_rate: 0.02
min_negative_control_abstention_rate: 0.98
```

---

## Task spec v1 shape

Existing task specs are close. Standardize them as:

```json
{
  "task_id": "code-bugfix-context-routing",
  "title": "Fix context routing regression",
  "prompt": "Diagnose and fix ...",
  "category": "bugfix",
  "difficulty": "medium",
  "surface_claim": "targeted_preflight",
  "negative_control": false,
  "stale_context_trap": false,
  "repo_commit": "<commit or fixture branch>",
  "time_budget_minutes": 20,
  "target_files": ["src/cmd/query.rs"],
  "target_symbols": ["task_spec_for_query"],
  "expected_relevant_files": ["src/cmd/query.rs"],
  "expected_validation_commands": [
    "cargo test -q cmd::query -- --nocapture",
    "cargo check --workspace --all-targets"
  ],
  "success_rubric": {
    "full_success": "...",
    "partial_success": "...",
    "failure": "...",
    "min_verification_quality": 4,
    "primary_endpoint": "verified_behavior_change"
  },
  "abstention_rubric": {
    "should_abstain": false,
    "unnecessary_context_definition": "Context is unnecessary only for negative controls."
  }
}
```

For negative controls:

```json
{
  "task_id": "negative-control-readme-typo",
  "title": "Fix a one-line typo without repo-wide context",
  "prompt": "Fix the spelling error explicitly identified in README.md line N.",
  "category": "negative_control",
  "negative_control": true,
  "surface_claim": "abstention",
  "target_files": ["README.md"],
  "expected_relevant_files": ["README.md"],
  "expected_validation_commands": ["git diff --check"],
  "success_rubric": {
    "full_success": "The typo is fixed and no broad context packet is generated or injected.",
    "partial_success": "The typo is fixed but unrelated context is inspected or injected.",
    "failure": "The typo is not fixed or unrelated context causes a regression.",
    "min_verification_quality": 2,
    "primary_endpoint": "abstention_and_correctness"
  },
  "abstention_rubric": {
    "should_abstain": true,
    "unnecessary_context_definition": "Any generated context packet above 500 tokens or any unrelated file inspection."
  }
}
```

---

## Benchmark variants

First implementation must support these variants distinctly:

1. `baseline`
   - no Layers packet
   - agent may inspect files/tools normally
2. `layers_targeted_preflight`
   - targeted `layers preflight --no-audit --json --strict --target ...`
   - packet must be validated and inspected
3. Optional in first pass, required before broad product claims: `layers_broad_query`
   - broad `layers query --json ...`
   - report separately; do not average into targeted preflight
4. Optional after MCP path stabilizes: `layers_mcp_preflight`

If the current Rust enum only supports `baseline` and `layers`, implement the expanded variants behind tests. Do not collapse broad query and targeted preflight into one `layers` bucket in future proof reports.

---

## Implementation phases

### Phase 0: Plan/document-only setup

**Objective:** Commit the proof design before touching benchmark behavior.

**Files:**
- Create: `docs/plans/2026-05-01-definitive-layers-proof-benchmark.md`
- Create later: `benchmarks/workflows/CLAIM_GATES.md`
- Modify later: `benchmarks/workflows/README.md`

**Steps:**
1. Save this plan.
2. Review for scope and proof target.
3. Commit with:
   ```sh
   git add docs/plans/2026-05-01-definitive-layers-proof-benchmark.md
   git commit -m "docs: plan definitive Layers proof benchmark"
   ```

**Verification:**
```sh
git diff --check
git status --short
```

---

### Phase 1: Preregister claim gates

**Objective:** Add explicit human-readable claim gates before generating more evidence.

**Files:**
- Create: `benchmarks/workflows/CLAIM_GATES.md`
- Modify: `benchmarks/workflows/README.md` if it exists; otherwise create it.

**Step 1: Write `CLAIM_GATES.md`**

Include:
- exact claim text
- primary endpoint: success delta
- secondary endpoints: tokens/time/tool calls/context quality
- failure conditions
- negative-control rules
- stale-context rules
- minimum sample sizes
- no-moving-goalposts rule

**Step 2: Add README explanation**

`benchmarks/workflows/README.md` should explain:
- how task specs work
- how runs are recorded
- how to run analysis
- what `supported`, `not_supported`, and `inconclusive` mean

**Step 3: Verify docs only**

```sh
git diff --check
git status --short
```

**Commit:**
```sh
git add benchmarks/workflows/CLAIM_GATES.md benchmarks/workflows/README.md
git commit -m "docs: preregister Layers proof claim gates"
```

---

### Phase 2: Add task-spec validation to the benchmark CLI

**Objective:** Make task specs machine-validated instead of informal JSON examples.

**Files:**
- Modify: `src/cmd/workflow_benchmark.rs`
- Add: `benchmarks/workflows/fixtures/valid-task-spec.json`
- Add: `benchmarks/workflows/fixtures/invalid-task-spec-missing-rubric.json`

**TDD RED test 1:** task specs deserialize and validate.

Add a test near existing workflow benchmark tests:

```rust
#[test]
fn task_spec_fixture_deserializes_and_validates() {
    let text = include_str!("../../benchmarks/workflows/fixtures/valid-task-spec.json");
    let spec: TaskSpec = serde_json::from_str(text).expect("deserialize task spec");
    spec.validate().expect("valid task spec");
    assert_eq!(spec.task_id, "fixture-valid-code-task");
}
```

Run:

```sh
cargo test -q task_spec_fixture_deserializes_and_validates -- --nocapture
```

Expected: fail because `TaskSpec` does not exist or fixture does not exist.

**GREEN implementation:**

Add minimal structs:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
struct TaskSpec {
    task_id: String,
    title: String,
    prompt: String,
    category: String,
    #[serde(default)]
    difficulty: Option<String>,
    #[serde(default)]
    surface_claim: Option<String>,
    #[serde(default)]
    negative_control: bool,
    #[serde(default)]
    stale_context_trap: bool,
    #[serde(default)]
    repo_commit: Option<String>,
    #[serde(default)]
    time_budget_minutes: Option<u64>,
    #[serde(default)]
    target_files: Vec<String>,
    #[serde(default)]
    target_symbols: Vec<String>,
    #[serde(default)]
    expected_relevant_files: Vec<String>,
    #[serde(default)]
    expected_validation_commands: Vec<String>,
    success_rubric: TaskSuccessRubric,
    #[serde(default)]
    abstention_rubric: Option<TaskAbstentionRubric>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TaskSuccessRubric {
    full_success: String,
    partial_success: String,
    failure: String,
    min_verification_quality: u8,
    #[serde(default)]
    primary_endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TaskAbstentionRubric {
    should_abstain: bool,
    unnecessary_context_definition: String,
}
```

Add `validate()` with minimal checks:
- non-empty `task_id`, `title`, `prompt`, `category`
- `min_verification_quality <= 5`
- negative controls must have an abstention rubric with `should_abstain = true`
- non-negative controls should not require abstention unless explicitly justified
- code-heavy categories should have expected relevant files and validation commands

**TDD RED test 2:** invalid fixture is rejected.

```rust
#[test]
fn invalid_task_spec_missing_rubric_is_rejected() {
    let text = include_str!("../../benchmarks/workflows/fixtures/invalid-task-spec-missing-rubric.json");
    let err = serde_json::from_str::<TaskSpec>(text).expect_err("missing rubric should fail");
    assert!(err.to_string().contains("success_rubric"));
}
```

Run specific test, then all workflow benchmark tests:

```sh
cargo test -q invalid_task_spec_missing_rubric_is_rejected -- --nocapture
cargo test -q workflow_benchmark -- --nocapture
```

**Commit:**
```sh
git add src/cmd/workflow_benchmark.rs benchmarks/workflows/fixtures
git commit -m "[verified] Validate workflow benchmark task specs"
```

---

### Phase 3: Add task corpus validation command

**Objective:** Add CLI support to validate all task specs under `benchmarks/workflows/tasks`.

**Files:**
- Modify: `src/cmd/workflow_benchmark.rs`
- Modify: `src/main.rs` only if command wiring requires parser test updates.

**Command target:**

```sh
layers workflow-benchmark validate-tasks benchmarks/workflows/tasks
layers workflow-benchmark validate-tasks benchmarks/workflows/tasks --json
```

**TDD RED test:** parser/handler accepts validate command.

Add command enum variant:

```rust
ValidateTasks {
    path: PathBuf,
    #[arg(long)]
    json: bool,
}
```

Test behavior with fixture temp dir if possible:

```rust
#[test]
fn validate_tasks_rejects_empty_task_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let err = validate_task_specs(dir.path()).expect_err("empty directory should fail");
    assert!(err.to_string().contains("no task specs"));
}
```

Run:

```sh
cargo test -q validate_tasks_rejects_empty_task_directory -- --nocapture
```

Expected: fail before implementation.

**GREEN implementation:**

Implement:
- read `*.json` in directory
- deserialize `TaskSpec`
- validate each
- count categories
- count negative controls
- fail if fewer than one normal and one negative-control task for now
- later claim gate enforces 30+ tasks

Human output example:

```text
Task specs: 30 valid
Categories:
  bugfix: 8
  feature: 6
  refactor: 5
  docs: 4
  negative_control: 5
  stale_context_trap: 2
```

JSON output example:

```json
{
  "task_count": 30,
  "valid": true,
  "negative_control_count": 5,
  "categories": {"bugfix": 8}
}
```

**Verification:**

```sh
cargo test -q workflow_benchmark -- --nocapture
cargo run -q -- workflow-benchmark validate-tasks benchmarks/workflows/tasks
```

**Commit:**
```sh
git add src/cmd/workflow_benchmark.rs src/main.rs
git commit -m "[verified] Add workflow task corpus validation"
```

---

### Phase 4: Split Layers variants by surface

**Objective:** Stop collapsing broad query and targeted preflight into one `layers` bucket.

**Files:**
- Modify: `src/cmd/workflow_benchmark.rs`
- Modify fixtures and dogfood examples if needed.

**New variants:**

```rust
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkflowVariant {
    Baseline,
    LayersBroadQuery,
    LayersTargetedPreflight,
    LayersMcpPreflight,
}
```

Backward compatibility option:
- accept legacy `layers` as alias for `layers_targeted_preflight` only in old dogfood artifacts, or
- keep legacy `Layers` variant but mark it as unsupported for new claim reports.

Preferred: support explicit variants and make legacy `layers` analyze as `layers_targeted_preflight` with a warning field.

**TDD RED tests:**

1. JSONL parser accepts `layers_targeted_preflight`.
2. Report compares `baseline` vs selected claim variant.
3. Broad query is reported separately and not averaged into targeted preflight.

Example test:

```rust
#[test]
fn broad_query_is_not_averaged_into_targeted_preflight_claim() {
    let runs = vec![
        parse_run(&valid_run_variant("task-1", "baseline", 1_000, 2_000)).unwrap(),
        parse_run(&valid_run_variant("task-1", "layers_targeted_preflight", 700, 1_200)).unwrap(),
        parse_run(&valid_run_variant("task-1", "layers_broad_query", 2_000, 5_000)).unwrap(),
    ];
    let report = analyze_runs_with_claim_variant(
        &runs,
        WorkflowVariant::LayersTargetedPreflight,
        ClaimThresholds::default(),
    ).expect("analysis");
    assert_eq!(report.comparison.as_ref().unwrap().paired_task_count, 1);
    assert!(report.comparison.as_ref().unwrap().net_time_saved_ms > 0.0);
}
```

**Verification:**

```sh
cargo test -q broad_query_is_not_averaged_into_targeted_preflight_claim -- --nocapture
cargo test -q workflow_benchmark -- --nocapture
cargo run -q -- workflow-benchmark analyze docs/dogfood/20260429T192051Z-layers-self-diagnostic/compare/workflow-runs.jsonl --json
```

**Commit:**
```sh
git add src/cmd/workflow_benchmark.rs
git commit -m "[verified] Separate Layers benchmark variants by surface"
```

---

### Phase 5: Add confidence intervals and inconclusive status

**Objective:** Avoid treating tiny sample sizes as definitive.

**Files:**
- Modify: `src/cmd/workflow_benchmark.rs`

**Data model change:**

Add:

```rust
enum ClaimStatus {
    Supported,
    NotSupported,
    Inconclusive,
}
```

Add confidence interval fields to comparison/report:

```rust
struct MetricInterval {
    estimate: f64,
    lower_95: f64,
    upper_95: f64,
}
```

At minimum compute bootstrap CIs for paired deltas:
- success delta
- time saved
- token reduction
- missed critical context delta

Use deterministic bootstrap seed for reproducibility. If avoiding RNG dependency, implement simple deterministic resampling from a fixed LCG or use exact paired deltas plus percentile over enumerated small samples where feasible. Keep it simple and tested.

**TDD RED tests:**

1. Reports with `paired_task_count < min_paired_tasks` are `inconclusive`, not `not_supported`, unless a hard safety blocker exists.
2. Confidence interval fields are present in JSON report when enough paired tasks exist.
3. CI values are finite.

Example:

```rust
#[test]
fn claim_is_inconclusive_when_sample_size_is_too_small() {
    let runs = vec![
        parse_run(&valid_run("task-1", "baseline", 1_000, 2_000)).unwrap(),
        parse_run(&valid_run("task-1", "layers_targeted_preflight", 700, 1_500)).unwrap(),
    ];
    let mut thresholds = ClaimThresholds::default();
    thresholds.min_paired_tasks = 30;
    let report = analyze_runs_with_thresholds(&runs, thresholds).expect("analysis");
    assert_eq!(report.claim.unwrap().status, ClaimStatus::Inconclusive);
}
```

**Verification:**

```sh
cargo test -q claim_is_inconclusive_when_sample_size_is_too_small -- --nocapture
cargo test -q workflow_benchmark -- --nocapture
```

**Commit:**
```sh
git add src/cmd/workflow_benchmark.rs
git commit -m "[verified] Add benchmark confidence and inconclusive claims"
```

---

### Phase 6: Build the 30-task corpus

**Objective:** Create the first preregistered task corpus large enough to attempt proof.

**Files:**
- Create/modify: `benchmarks/workflows/tasks/*.json`
- Modify: `benchmarks/workflows/README.md`

**Corpus target:**

30 tasks minimum:

```text
8 bugfix tasks
6 small feature tasks
5 refactor tasks
4 documentation/architecture tasks
2 stale-context trap tasks
5 negative controls
```

Every task must include:
- fixed prompt
- category
- target files if targeted preflight should be used
- expected relevant files
- validation commands
- success rubric
- abstention rubric for negative controls

**Suggested task IDs:**

Bugfix:
- `code-bugfix-context-routing`
- `code-bugfix-mcp-preflight-surface`
- `code-bugfix-packet-validation-warning`
- `code-bugfix-workflow-claim-gate`
- `code-bugfix-query-target-detection`
- `code-bugfix-telemetry-side-effect`
- `code-bugfix-cli-parse-regression`
- `code-bugfix-context-budget-overflow`

Feature:
- `feature-workflow-task-validation`
- `feature-packet-artifact-summary`
- `feature-preflight-target-coverage-report`
- `feature-workflow-markdown-report`
- `feature-claim-threshold-config`
- `feature-mcp-tool-list-check`

Refactor:
- `refactor-contextcompiler-shared-helper`
- `refactor-workflow-report-formatting`
- `refactor-query-preflight-task-spec`
- `refactor-packet-quality-test-fixtures`
- `refactor-cli-command-wiring`

Docs/architecture:
- `docs-roadmap-evidence-gates`
- `docs-context-packet-contract`
- `docs-mcp-stable-surface`
- `docs-workflow-benchmark-howto`

Stale context traps:
- `stale-trap-current-code-over-memory-routing`
- `stale-trap-renamed-file-grounding`

Negative controls:
- `negative-control-trivial-arithmetic`
- `negative-control-readme-typo`
- `negative-control-cargo-fmt-only`
- `negative-control-explain-pasted-function`
- `negative-control-single-file-variable-rename`

**Verification:**

```sh
cargo run -q -- workflow-benchmark validate-tasks benchmarks/workflows/tasks
```

**Commit:**
```sh
git add benchmarks/workflows/tasks benchmarks/workflows/README.md
git commit -m "bench: add preregistered Layers proof task corpus"
```

---

### Phase 7: Define manual/agent run protocol

**Objective:** Make runs reproducible before automating them.

**Files:**
- Create: `benchmarks/workflows/RUN_PROTOCOL.md`
- Create: `benchmarks/workflows/templates/workflow-run-record.json`
- Create: `benchmarks/workflows/templates/transcript-template.md`

**Protocol must specify:**

1. Checkout/reset procedure.
2. Agent/model used.
3. Tool permissions.
4. Time budget.
5. Randomized order.
6. Baseline prompt format.
7. Targeted preflight prompt format.
8. How to save packet artifacts.
9. How to score success.
10. How to record tokens/tool calls/time.
11. How to classify missed critical context.
12. How to classify stale context.
13. How to classify unnecessary context injection.

**Baseline prompt template:**

```text
You are evaluating a benchmark task without Layers context.
Task ID: <task_id>
Task: <prompt>
Allowed repo: /Users/xxx/layers
Rules:
- Do not run `layers query`, `layers preflight`, or use generated Layers packets.
- Complete the task or explain why it cannot be completed.
- Run the expected validation commands if you change code.
- Save transcript notes to <artifact_dir>/transcripts/baseline/<task_id>.txt.
```

**Targeted preflight prompt template:**

```text
You are evaluating a benchmark task with targeted Layers preflight.
Task ID: <task_id>
Task: <prompt>
Before implementation, run:
  cargo run -q -- preflight --no-audit --json --strict <targets> "<prompt>" > <packet_path>
Then validate and inspect the packet:
  cargo run -q -- packet validate <packet_path>
  cargo run -q -- packet inspect --json <packet_path> > <inspect_path>
Use only relevant packet content. If the packet is weak/stale/misleading, record that and proceed carefully.
Run expected validation commands if you change code.
Save transcript notes to <artifact_dir>/transcripts/layers-targeted-preflight/<task_id>.txt.
```

**Commit:**
```sh
git add benchmarks/workflows/RUN_PROTOCOL.md benchmarks/workflows/templates
git commit -m "docs: define Layers proof benchmark run protocol"
```

---

### Phase 8: Execute first full benchmark run

**Objective:** Produce a full local artifact set for baseline vs targeted preflight.

**Files:**
- Create: `docs/dogfood/<timestamp>-layers-vs-baseline-proof/...`

**Execution checklist:**

For each task:

1. Reset working tree.
   ```sh
   git checkout -- .
   git clean -fd
   ```
   Be careful not to delete intended benchmark artifacts; run from a separate worktree if necessary.

2. Run baseline variant.
3. Save transcript.
4. Save validation logs.
5. Record JSONL row.
6. Reset working tree.
7. Run targeted preflight variant.
8. Save packet, validation, inspect output.
9. Save transcript.
10. Save validation logs.
11. Record JSONL row.

Prefer using a throwaway git worktree for each run:

```sh
git worktree add /tmp/layers-bench-<task_id>-baseline HEAD
git worktree add /tmp/layers-bench-<task_id>-layers HEAD
```

**Analyze:**

```sh
cargo run -q -- workflow-benchmark analyze docs/dogfood/<timestamp>-layers-vs-baseline-proof/compare/workflow-runs.jsonl --json \
  > docs/dogfood/<timestamp>-layers-vs-baseline-proof/compare/workflow-benchmark-report.json

cargo run -q -- workflow-benchmark analyze docs/dogfood/<timestamp>-layers-vs-baseline-proof/compare/workflow-runs.jsonl \
  > docs/dogfood/<timestamp>-layers-vs-baseline-proof/compare/workflow-benchmark-report.md
```

**Create `CLAIM.md`:**

Must include:
- result status
- blocking metrics
- exact claim supported or not
- top failures
- next product fixes

**Do not commit generated artifacts until secret scan passes.**

---

### Phase 9: Artifact/static scan and report review

**Objective:** Ensure benchmark artifacts are safe and credible.

**Files:**
- Generated dogfood directory only.

**Secret scan:**

```sh
python3 - <<'PY'
from pathlib import Path
import re
root = Path('/Users/xxx/layers/docs/dogfood')
patterns = [
    re.compile(r'sk-[A-Za-z0-9_-]{16,}'),
    re.compile(r'(?i)(api[_-]?key|secret|password|passwd)\s*[:=]\s*["\'][^"\']{8,}["\']'),
]
for file in root.rglob('*'):
    if file.is_file():
        text = file.read_text(errors='ignore')
        for pattern in patterns:
            for match in pattern.finditer(text):
                line = text.count('\n', 0, match.start()) + 1
                print(f'{file}:{line}:{match.group(0)[:80]}')
PY
```

Expected: no output.

**Review checklist:**

- Every task has paired runs.
- No baseline run used Layers context.
- Every Layers run saved packet artifacts.
- Every packet was validated.
- Negative controls are scored harshly for unnecessary context.
- Stale-context traps are scored harshly for stale memory use.
- Success scoring is based on validation, not subjective vibes.
- `CLAIM.md` does not overclaim.

**Independent review:**

Use `delegate_task` to ask a reviewer to inspect:
- task corpus cherry-picking risk
- claim gate adequacy
- scoring consistency
- whether artifacts support the claim
- whether broad query is accidentally mixed into targeted preflight
- whether generated artifacts contain secrets

---

### Phase 10: Product fix loop if claim fails

**Objective:** Use benchmark failures to drive product work without moving the proof target.

If claim fails due to success delta:
- inspect failed task transcripts
- identify whether Layers caused failure or simply failed to help
- add focused regression tests before changing code

If claim fails due to missed critical context:
- improve target discovery or packet completeness checks
- add tests for missing expected relevant files

If claim fails due to stale context:
- add freshness/current-code precedence checks
- add stale-context warning codes

If claim fails due to negative controls:
- add abstention classifier/gate
- cap context packet size for trivial tasks
- make `preflight` refuse/abstain when expected useful context is near zero

If claim fails due to overhead:
- cache packet generation
- reduce retrieval fanout
- make targeted preflight cheaper

After fixes:
- do not change `CLAIM_GATES.md` unless making gates stricter
- rerun the same preregistered corpus
- compare before/after product changes separately

---

## Full verification gates for code phases

Before each `[verified]` code commit:

```sh
cargo fmt --all --check
git diff --check
cargo check --no-default-features --all-targets
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

If full tests hit the known flaky `layers-tools process::tests::process_tool_poll_updates_status`, rerun that targeted test and then rerun full workspace tests before claiming the gate passed.

After tests, revert runtime telemetry side effects unless intentionally included:

```sh
git checkout -- memoryport/telemetry/events.jsonl || true
git status --short
```

---

## Definition of done

The plan is fully implemented when the repo contains:

1. `benchmarks/workflows/CLAIM_GATES.md` with preregistered gates.
2. `layers workflow-benchmark validate-tasks` or equivalent validation.
3. 30+ validated task specs, including 5+ negative controls.
4. Benchmark analysis that separates baseline, broad query, targeted preflight, and MCP preflight variants.
5. Confidence intervals or equivalent uncertainty reporting.
6. A complete dogfood proof run directory with raw artifacts.
7. A `CLAIM.md` that honestly says `supported`, `not_supported`, or `inconclusive`.
8. Independent review of both the code/reporting and the benchmark artifacts.

The project only gets to say “definitive proof” for the exact claim that passes the gates. If targeted preflight passes but broad query fails, the claim must remain targeted-preflight-specific.

---

## Recommended execution order

1. Commit this plan.
2. Add `CLAIM_GATES.md` and workflow README.
3. Add task-spec validation.
4. Add corpus validation command.
5. Split benchmark variants by Layers surface.
6. Add inconclusive status and confidence intervals.
7. Build 30-task corpus.
8. Write run protocol/templates.
9. Run full paired benchmark.
10. Review artifacts and only then state the claim.
