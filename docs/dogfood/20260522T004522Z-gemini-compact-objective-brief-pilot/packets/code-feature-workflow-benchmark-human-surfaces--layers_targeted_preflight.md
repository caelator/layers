# Objective Brief

## Objective

List all benchmark surfaces in human reports

Improve human workflow benchmark reports so each Layers surface and comparison variant is named explicitly rather than relying on a legacy singular Layers summary. Keep JSON output backward compatible.

## Context Constraints

- packet: preflight-60005cce-cbc4-42f6-8b86-25512409d4a5
- workspace: code-feature-workflow-benchmark-human-surfaces--layers_targeted_preflight
- route: preflight
- confidence: high
- budget: 212/700 words
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
  source: /Users/xxx/layers/docs/dogfood/20260522T004522Z-gemini-compact-objective-brief-pilot/worktrees/code-feature-workflow-benchmark-human-surfaces--layers_targeted_preflight/memoryport/curated-memory.jsonl (memory)
  selected because: curated memory shares terms with the preflight task
  tags: memory
- Project Memory / Curated memory match
  source: /Users/xxx/layers/docs/dogfood/20260522T004522Z-gemini-compact-objective-brief-pilot/worktrees/code-feature-workflow-benchmark-human-surfaces--layers_targeted_preflight/memoryport/curated-memory.jsonl (memory)
  selected because: curated memory shares terms with the preflight task
  tags: memory
- Code and Impact Context / src/cmd/workflow_benchmark.rs
  source: src/cmd/workflow_benchmark.rs (file) [src/cmd/workflow_benchmark.rs]
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

