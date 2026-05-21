# Objective Brief

## Objective

Avoid committing generated telemetry artifacts and avoid expanding deprecated runtime/MCP surfaces while improving Layers release readiness

## Context Constraints

- packet: preflight-e5115a81-8e38-4e11-8552-16ad6e3a815a
- workspace: layers
- route: preflight
- confidence: high
- budget: 289/700 words
- git ref: ba3664c

## Warnings

- [warning] dirty_worktree: Workspace has 2 changed and 1 untracked files; inspect before editing.
- [info] autoresearch_uncertainty: No persisted autoresearch finding matched this task.
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
  source: /Users/xxx/layers/memoryport/curated-memory.jsonl (memory)
  selected because: curated memory shares terms with the preflight task
  tags: memory
- Project Memory / Curated memory match
  source: /Users/xxx/layers/memoryport/curated-memory.jsonl (memory)
  selected because: curated memory shares terms with the preflight task
  tags: memory
- Code and Impact Context / docs/V2_PRODUCT_CONTRACT.md
  source: docs/V2_PRODUCT_CONTRACT.md (file) [docs/V2_PRODUCT_CONTRACT.md]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / src/cmd/preflight.rs
  source: src/cmd/preflight.rs (file) [src/cmd/preflight.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / src/cmd/workflow_benchmark.rs
  source: src/cmd/workflow_benchmark.rs (file) [src/cmd/workflow_benchmark.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
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

