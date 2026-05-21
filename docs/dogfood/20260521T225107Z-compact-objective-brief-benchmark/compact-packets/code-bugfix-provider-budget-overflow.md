# Objective Brief

## Objective

Audit provider budget accounting for saturating arithmetic and f64/u64 conversions. Add a regression test for very large token counters without panics or wraparound.

## Context Constraints

- packet: preflight-1838f656-d070-4421-a4ff-9da58bdb39bc
- workspace: layers
- route: preflight
- confidence: high
- budget: 401/700 words
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
- Code and Impact Context / src/provider/accounting.rs
  source: src/provider/accounting.rs (file) [src/provider/accounting.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / src/provider/tokenizer.rs
  source: src/provider/tokenizer.rs (file) [src/provider/tokenizer.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / crates/layers-providers/src/token_accounting.rs
  source: crates/layers-providers/src/token_accounting.rs (file) [crates/layers-providers/src/token_accounting.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Impact Context / src/provider/accounting.rs
  source: impact:src/provider/accounting.rs (gitnexus) [src/provider/accounting.rs]
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
