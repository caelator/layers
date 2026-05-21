# Objective Brief

## Objective

Diagnose and fix a regression in Layers context routing where code-heavy queries with explicit Rust targets should request or inject targeted code context instead of falling back to memory-only context. Use tests to prove the routing behavior and run the relevant Rust validation gates.

## Context Constraints

- packet: preflight-82a78da8-a939-4e2c-89d8-fb704554d3b2
- workspace: layers
- route: preflight
- confidence: high
- budget: 324/700 words
- git ref: ca45a9b

## Warnings

- [warning] dirty_worktree: Workspace has 7 changed and 4 untracked files; inspect before editing.
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
- Code and Impact Context / src/cmd/query.rs
  source: src/cmd/query.rs (file) [src/cmd/query.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / src/cmd/preflight.rs
  source: src/cmd/preflight.rs (file) [src/cmd/preflight.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / crates/layers-core/src/packet_quality.rs
  source: crates/layers-core/src/packet_quality.rs (file) [crates/layers-core/src/packet_quality.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Impact Context / src/cmd/query.rs
  source: impact:src/cmd/query.rs (gitnexus) [src/cmd/query.rs]
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
- Suggested Validation Commands / ./target/debug/layers query --no-audit --json "What should I know before editing src/cmd/query.rs?" | python3 -m json.tool >/dev/null
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
