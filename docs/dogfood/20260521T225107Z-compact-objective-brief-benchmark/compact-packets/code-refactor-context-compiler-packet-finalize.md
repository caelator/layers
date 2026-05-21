# Objective Brief

## Objective

Refactor packet finalization paths so packet id, created_at, provenance, and stable metadata are set through ContextCompiler rather than duplicated ad hoc call sites. Preserve existing query and preflight behavior.

## Context Constraints

- packet: preflight-44c71014-ec2e-4202-a093-1c54c3fda835
- workspace: layers
- route: preflight
- confidence: high
- budget: 346/700 words
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
- Code and Impact Context / src/context_packet_compiler/mod.rs
  source: src/context_packet_compiler/mod.rs (file) [src/context_packet_compiler/mod.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / src/cmd/query.rs
  source: src/cmd/query.rs (file) [src/cmd/query.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / src/cmd/preflight.rs
  source: src/cmd/preflight.rs (file) [src/cmd/preflight.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Impact Context / src/context_packet_compiler/mod.rs
  source: impact:src/context_packet_compiler/mod.rs (gitnexus) [src/context_packet_compiler/mod.rs]
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
