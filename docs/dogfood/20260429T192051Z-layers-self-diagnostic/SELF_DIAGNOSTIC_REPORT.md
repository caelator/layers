# Layers self-diagnostic report — 20260429T192051Z

## Scope

Task: run a full debug and self diagnostic of Layers, verify it is functional, and compare evidence against using no Layers context.

This report separates three claims:

1. Functional health: whether the current repo builds, lints, tests, and exposes the expected CLI surfaces.
2. Dogfood evidence: whether Layers can produce valid context packets for its own repo.
3. Comparative evidence: whether those packets are better than using no Layers context for this diagnostic session.

## Functional health verdict

Verdict: PASS for the current local diagnostic gates.

Evidence:

- `cargo fmt --all --check`: exit 0
- `cargo check --no-default-features --all-targets`: exit 0
- `cargo check --workspace --all-targets`: exit 0
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0
- `git diff --check`: exit 0
- `cargo test --workspace --all-targets`: exit 0 on rerun

The earlier full workspace test failure was not reproduced. The isolated suspected flaky test `layers-tools process::tests::process_tool_poll_updates_status` passed, and the subsequent full workspace run passed.

Gate artifacts are in `gates/`.

## Dogfood verdict

Verdict: PASS, with one important quality caveat.

Layers generated and validated context packet artifacts for its own repo:

- `context-packet.json`: query route, valid packet, low confidence, memory-only fallback.
- `targeted-context-packet.json`: query route, valid packet, low confidence, memory-only fallback.
- `structural-context-packet.json`: query route, valid packet, high confidence but still memory-only fallback.
- `preflight-targeted.json`: preflight route, valid packet, high confidence, included workspace, memory, code, and validation sections.

All generated packets passed `layers packet validate` and `layers packet inspect --json`.

Quality caveat: broad `layers query` was not good enough for this task. It fell back to memory-only retrieval and warned that memory relevance was low. The targeted `layers preflight --target ... --strict` path was materially better: it retrieved the target files and suggested the exact validation commands needed for the code-heavy workflow.

Dogfood artifact summary is in `dogfood/context-artifact-summary.json`.

## Comparative verdict against using no Layers context

Verdict: mixed; do not claim general superiority from this run.

Observed evidence supports this narrower claim:

- Targeted `layers preflight --target src/cmd/workflow_benchmark.rs --target src/main.rs --target src/cmd/mod.rs --strict ...` was better than manual/no-Layers orientation for the workflow-benchmark diagnostic subtask because it produced a compact, valid, high-confidence packet with:
  - explicit target files,
  - workspace dirty-state warning,
  - relevant project memory,
  - code excerpts from the target files,
  - required validation commands.

Observed evidence does not support the broad claim that Layers is always better than nothing:

- Broad `layers query` for the whole self-diagnostic task produced low-confidence memory-only fallback context.
- The broad packet was concise and valid, but it missed code-heavy context and explicitly warned about low retrieval quality.

Microbenchmark result from `workflow-benchmark analyze compare/workflow-runs.jsonl`:

- paired tasks: 2
- net time saved: 60000 ms
- net tokens saved: 1575
- speedup: 1.4x
- token reduction ratio: 0.5625
- success delta: -0.25
- tool call delta: -5.5
- verification quality delta: +0.5
- context quality delta: +0.642857142857143
- missed critical context rate: 0.5
- hallucinated/stale context rate: 0.0

Interpretation: targeted preflight improved context quality and efficiency; broad query harmed success on one paired task because it returned low-confidence, incomplete context. Layers is functional and useful when invoked through the right surface, but the product should route code-heavy diagnostic/editing tasks toward preflight rather than broad query.

Comparative artifacts are in `compare/`.

## Product issues found

1. Broad query can be too weak for code-heavy diagnostics.
   - Evidence: `context-packet.json`, `targeted-context-packet.json`, and `structural-context-packet.json` used `memory_only` and warned about low relevance or fallback.
   - Impact: a user asking for a self-diagnostic may need code, validation, and workspace context; broad query alone under-serves that.

2. Targeted preflight is the strongest current surface for code-heavy work.
   - Evidence: `preflight-targeted.json` was high confidence, included code and validation sections, used 802 words, and passed strict mode.

3. UC semantic retrieval timed out in query runs.
   - Evidence: query packets reported `fallback_reason: uc timed out after 500ms` with memory latency around 523–526 ms.
   - Impact: this degrades query quality and increases fallback reliance.

## Files changed by this diagnostic session

Intended artifacts:

- `docs/dogfood/20260429T192051Z-layers-self-diagnostic/`

Previously implemented benchmark feature files still modified from the implementation task:

- `src/cmd/mod.rs`
- `src/main.rs`
- `src/cmd/workflow_benchmark.rs`

Runtime telemetry was reverted after diagnostic commands:

- `memoryport/telemetry/events.jsonl` is not left modified.

## Recommended next steps

1. Treat `layers preflight --strict --target ...` as the required self-dogfood surface for code-heavy tasks.
2. Improve broad query routing so code-heavy diagnostic phrases trigger preflight-style code/validation context or clearly advise using preflight.
3. Investigate UC timeout/fallback behavior if query quality matters for this workflow.
4. Commit the workflow benchmark feature and this dogfood artifact set separately if the artifacts should become part of the repo history.
