# Codex Project Manager Progress

## What Landed

This round established the first Layers-native project/task slice:

- Wrote the architecture and implementation plan in `codex-project-manager-plan.md`.
- Added canonical typed entities for:
  - `Project`
  - `Task`
  - `Decision`
  - `Constraint`
  - `StatusRecord`
  - `NextStep`
  - `Postmortem`
  - `ProjectRecord` envelope + typed payload enum
- Added a canonical local store:
  - `memoryport/project-records.jsonl`
- Added a new `src/projects.rs` module with:
  - append/load helpers
  - slug normalization
  - project creation/listing
  - task creation/listing
  - structured search for project/task records
  - record-shape validation
- Added initial CLI commands:
  - `layers project create`
  - `layers project list`
  - `layers task create`
  - `layers task list`
- Integrated structured project/task retrieval into `search_memory()` as the first memory tier.

## Intentional Scope Limit

I did not try to build a full PM system. The implemented slice is:

- canonical local records
- minimal creation/listing commands
- retrieval visibility through existing Layers query flow

I deferred:

- update/archive flows
- task dependencies
- structured `remember decision|constraint|status|next_step|postmortem`
- workflow packet persistence for plan/handoff/postmortem
- Memoryport indexing of derived retrieval text

## Impact Analysis

I ran GitNexus impact checks before editing the main symbols likely to move:

- `handle_remember`: `LOW`
- `handle_query`: `LOW`
- `main`: `LOW`
- `search_memory`: `HIGH`
- `council_files`: `HIGH`
- `MemoryHit`: `HIGH`

The high-risk result was on the retrieval path, so the implementation stayed additive:

- routing was not changed
- unstructured semantic/fallback retrieval was kept intact
- structured records were prepended as an extra retrieval tier

## Validation

Completed:

- `cargo fmt --all`
- `cargo test`
- CLI smoke test in a temporary workspace root:
  - `layers project create ...`
  - `layers task create ...`
  - `layers project list`
  - `layers task list --project ...`

Observed result:

- build/test passed
- the new commands wrote/read `memoryport/project-records.jsonl` correctly in a temp workspace
- structured records render as `MemoryHit` candidates for query-time retrieval

## GitNexus Change Scope Note

The repo instructions call for `gitnexus_detect_changes()`, but that MCP tool is not available in this Codex environment. I used:

- GitNexus `impact` and `context`
- `git status --short`
- build/test/smoke validation

as the available fallback checks.

## Recommended Next Slice

1. Extend `layers remember` to write `decision`, `constraint`, `status`, `next_step`, and `postmortem` into the canonical project record store.
2. Add `task show` and `project show` views that assemble a useful current-state packet from canonical records.
3. Add update/supersession semantics so status and next steps can evolve without losing auditability.
4. Teach synthesis to summarize structured project records explicitly instead of only surfacing them as generic memory sources.
