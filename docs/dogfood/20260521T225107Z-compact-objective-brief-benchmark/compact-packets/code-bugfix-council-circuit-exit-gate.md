# Objective Brief

## Objective

Diagnose and fix an edge case where the council circuit breaker exit gate could reopen too early or too late around threshold boundaries. Prove behavior with boundary tests.

## Context Constraints

- packet: preflight-dc10b100-b71c-4a86-b081-3ac9c9faa34a
- workspace: layers
- route: preflight
- confidence: high
- budget: 319/700 words
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
- Code and Impact Context / src/council/circuit_breaker.rs
  source: src/council/circuit_breaker.rs (file) [src/council/circuit_breaker.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / src/council/topology.rs
  source: src/council/topology.rs (file) [src/council/topology.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Impact Context / src/council/circuit_breaker.rs
  source: impact:src/council/circuit_breaker.rs (gitnexus) [src/council/circuit_breaker.rs]
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
