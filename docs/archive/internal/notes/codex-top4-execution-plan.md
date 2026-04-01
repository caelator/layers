# Codex Top 4 Execution Plan

## Narrowed Goal

Advance the top four directions inside the existing Rust Layers prototype without widening into plugin/platform work:

1. make curated memory more first-class
2. enforce tighter architecture-summary discipline
3. make GitNexus more workflow-operational
4. preserve typed boundaries that fit the v2 architecture direction

## Smallest Meaningful Slice

Implement one tight vertical slice across retrieval, synthesis, and validation:

- replace heuristic free-text "decision signal" extraction with typed memory-brief synthesis driven by stable `kind` values already present in structured/project records
- prefer structured/project-record hits more aggressively during ranking and formatting so curated knowledge surfaces ahead of looser memory
- add a compact `architecture_summary` output to `build_context` so queries produce a short, disciplined summary before raw evidence
- extend `validate` to check the new typed memory brief path and run a GitNexus workflow smoke query, making graph support more operational than a passive status check

## Why This Slice

- It directly improves the highest-value path: `query -> search_memory -> build_context -> validate`.
- It uses existing storage and provider surfaces, so the change stays small.
- It moves the system toward first-class curated memory and typed output without forcing a larger provider refactor yet.
- It creates clear staging for later work: dedicated curated storage, provider traits, and richer GitNexus plan/impact artifacts.

## Implementation Steps

1. Add a typed `MemoryBrief` struct for durable synthesis output shape.
2. Rework `synthesize_memory_brief` to categorize hits by `kind` instead of keyword guessing.
3. Strengthen memory ranking so structured/project-record hits are preferred in mixed retrieval.
4. Add `architecture_summary` generation inside `build_context`.
5. Extend `validate` with typed-memory and GitNexus workflow smoke checks.
6. Add regression tests for typed memory synthesis and context rendering.

## Explicit Non-Goals In This Pass

- no plugin platform
- no generic workflow engine
- no new provider trait abstraction yet
- no storage migration to a new curated JSONL file
- no autonomous writeback or graph-driven editing

## Staged Next

- introduce a dedicated curated-memory store and CLI flows for explicit `decision` / `constraint` / `status` / `next_step` promotion
- split current subprocess integrations behind typed provider adapters
- add GitNexus-backed impact/plan validation artifacts instead of only query-time and validate-time summaries
- normalize workflow artifacts further around `workflow.plan`, `workflow.handoff`, and `workflow.postmortem`
