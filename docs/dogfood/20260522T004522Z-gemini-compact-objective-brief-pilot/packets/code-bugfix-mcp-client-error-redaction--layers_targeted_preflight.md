# Objective Brief

## Objective

Redact MCP client error payload secrets

Ensure MCP client/server error formatting never echoes secret-like request payload fields. Add tests that secret-like values are redacted in errors.

## Context Constraints

- packet: preflight-d3ab65bd-b7c9-4456-8ae7-b8487419b2c4
- workspace: code-bugfix-mcp-client-error-redaction--layers_targeted_preflight
- route: preflight
- confidence: high
- budget: 345/700 words
- git ref: a7217f5

## Warnings

- [info] autoresearch_uncertainty: No persisted autoresearch finding matched this task.
- [warning] impact_degraded: Impact analysis for crates/layers-mcp/src/client.rs degraded; GitNexus context was unavailable or incomplete.
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
  source: /Users/xxx/layers/docs/dogfood/20260522T004522Z-gemini-compact-objective-brief-pilot/worktrees/code-bugfix-mcp-client-error-redaction--layers_targeted_preflight/memoryport/curated-memory.jsonl (memory)
  selected because: curated memory shares terms with the preflight task
  tags: memory
- Project Memory / Curated memory match
  source: /Users/xxx/layers/docs/dogfood/20260522T004522Z-gemini-compact-objective-brief-pilot/worktrees/code-bugfix-mcp-client-error-redaction--layers_targeted_preflight/memoryport/curated-memory.jsonl (memory)
  selected because: curated memory shares terms with the preflight task
  tags: memory
- Code and Impact Context / crates/layers-mcp/src/client.rs
  source: crates/layers-mcp/src/client.rs (file) [crates/layers-mcp/src/client.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / crates/layers-mcp/src/server.rs
  source: crates/layers-mcp/src/server.rs (file) [crates/layers-mcp/src/server.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Code and Impact Context / crates/layers-mcp/src/types.rs
  source: crates/layers-mcp/src/types.rs (file) [crates/layers-mcp/src/types.rs]
  selected because: explicit or inferred target file should be inspected before editing
  tags: code
- Impact Context / crates/layers-mcp/src/client.rs
  source: impact:crates/layers-mcp/src/client.rs (local) [crates/layers-mcp/src/client.rs]
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

