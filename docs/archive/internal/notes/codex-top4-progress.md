# Codex Top 4 Progress

## Implemented Now

### Curated memory first-classing

- Added typed `MemoryBrief` output in `src/types.rs`.
- Replaced brittle keyword-scanned decision extraction in `src/memory.rs` with typed synthesis based on `MemoryHit.kind`.
- Strengthened ranking so `project-records` hits outrank semantic/fallback hits when relevance is otherwise comparable.
- Marked structured hits as `preferred` in formatted memory-source output.

### Architecture summary discipline

- Added `architecture_summary` generation in `src/synthesis.rs`.
- `build_context` now emits a compact summary before the longer evidence section.
- Query JSON payload now includes both `architecture_summary` and structured `memory_brief`.

### GitNexus more workflow-operational

- `handle_validate` now runs a GitNexus query smoke check, not just index/status inspection.
- Validation output now reports `graph_workflow.results` / `graph_workflow.issue`.

### Typed/stable boundaries

- The new summary path is typed and serializable rather than being implicit string heuristics.
- This keeps the surface aligned with the v2 direction: stable output contracts, replaceable internals.

## Verification

- `cargo fmt`
- `cargo test -q`
- `cargo run --quiet -- validate`

## Validation Notes

- Tests passed: `9 passed; 0 failed`
- `validate` passed overall.
- Memory provider replacement is still not ready in this environment because semantic embedding calls to `http://localhost:11434/api/embed` are blocked, so `memory_provider.ok` remained `false`.
- GitNexus workflow smoke query returned `3` results, so the new operational check is live.

## Staged Next

- add dedicated curated-memory persistence instead of relying on existing structured/project records
- promote explicit decision/constraint/status/next_step write paths through CLI commands
- turn GitNexus impact and plan checks into first-class workflow artifacts rather than validate-only smoke checks
- continue shrinking ad hoc logic in `commands.rs` behind typed workflow/provider boundaries

## Scope Check

- Code changes stayed inside `src/memory.rs`, `src/synthesis.rs`, `src/commands.rs`, `src/types.rs`, and tests in `src/main.rs`.
- No plugin-system, workflow-engine, or provider-abstraction expansion was introduced in this pass.
