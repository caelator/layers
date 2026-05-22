# Objective Brief

## Objective

Prefer current task_spec schema over stale docs

Stale-context trap: update code using the current TaskSpec validation rules, not older docs that allowed missing expected relevant files. The correct solution must inspect current schema and validator before editing.

## Context Constraints

- packet: preflight-e529d35b-7fd3-41a0-9e8f-7ed8227665f0
- workspace: code-stale-trap-prefer-current-task-spec--layers_targeted_preflight
- route: preflight
- confidence: high
- budget: 249/700 words
- git ref: a7217f5

## Warnings

- [info] autoresearch_uncertainty: No persisted autoresearch finding matched this task.
- [warning] impact_degraded: Impact analysis for src/cmd/workflow_benchmark.rs degraded; GitNexus context was unavailable or incomplete.
- [info] injection_policy: Packet quality gate recommends InjectFull: packet has strong target coverage and low warning burden

## Cited Context

- Preflight Summary / Preflight classification
  source: local-planner (preflight)
  selected because: summarizes the local preflight plan before source collection
  tags: planning
- Workspace State / Git worktree state
  source: git status --porcelain=v1 (workspace)
  selected because: dirty or divergent workspace state changes how agents should interpret context
  tags: workspace, git
- Project Memory / Curated memory match
  source: /Users/xxx/layers/docs/dogfood/20260522T004522Z-gemini-compact-objective-brief-pilot/worktrees/code-stale-trap-prefer-current-task-spec--layers_targeted_preflight/memoryport/curated-memory.jsonl (memory)
  selected because: curated memory shares terms with the preflight task
  tags: memory
- Project Memory / Curated memory match
  source: /Users/xxx/layers/docs/dogfood/20260522T004522Z-gemini-compact-objective-brief-pilot/worktrees/code-stale-trap-prefer-current-task-spec--layers_targeted_preflight/memoryport/curated-memory.jsonl (memory)
  selected because: curated memory shares terms with the preflight task
  tags: memory
- Code and Impact Context / src/cmd/workflow_benchmark.rs
  source: src/cmd/workflow_benchmark.rs (file) [src/cmd/workflow_benchmark.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / benchmarks/workflows/schemas/task-spec.schema.json
  source: benchmarks/workflows/schemas/task-spec.schema.json (file) [benchmarks/workflows/schemas/task-spec.schema.json]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Impact Context / src/cmd/workflow_benchmark.rs
  source: impact:src/cmd/workflow_benchmark.rs (local) [src/cmd/workflow_benchmark.rs]
  selected because: impact context identifies likely blast radius and validation before editing
  tags: impact
- Suggested Validation Commands / git diff --check
  source: preflight-validation-policy (validation)
  selected because: validation commands prevent context-only research from becoming unverified implementation
  tags: validation
- Suggested Validation Commands / cargo test --workspace --all-targets
  source: preflight-validation-policy (validation)
  selected because: validation commands prevent context-only research from becoming unverified implementation
  tags: validation
- Suggested Validation Commands / cargo clippy --workspace --all-targets -- -D warnings
  source: preflight-validation-policy (validation)
  selected because: validation commands prevent context-only research from becoming unverified implementation
  tags: validation

## Validation Plan

No explicit validation commands were captured in this packet.

## Handoff Expectations

- Use the cited context above as the task boundary.
- Do not assume uncited repository facts are current.
- Preserve packet citations in review notes when they justify edits.
- If the cited context is insufficient, stop and request an updated ContextPacket or explicit targets.

