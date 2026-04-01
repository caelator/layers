# Layers-Native Project/Task Subsystem Plan

## Goal

Build a local-first project/task coordination layer that extends Layers' existing memory and workflow model instead of introducing a separate PM product. The subsystem should treat structured records as the canonical truth and let retrieval/synthesis consume derived summaries.

## Design Principles

- Keep canonical state in local structured records under `memoryport/`.
- Reuse the existing Layers ergonomics: explicit CLI writes, auditable retrieval, append-friendly storage.
- Prefer a small stable schema over a large workflow engine.
- Model project coordination as durable records and packets that agents can create, read, and reuse.
- Preserve space for later planning, handoff, review, postmortem, and council workflows.

## Canonical Model

Use one append-only record stream for project/task coordination:

`memoryport/project-records.jsonl`

Each entry carries a common envelope plus one typed entity payload.

### Envelope

```jsonc
{
  "id": "pm_20260331T210000Z_project_layers-native-pm",
  "entity": "project",              // project | task | decision | constraint | status | next_step | postmortem
  "project": "layers-native-pm",
  "task": null,
  "created_at": "2026-03-31T21:00:00Z",
  "source": "manual",               // manual | agent | backfill
  "tags": ["layers", "project-manager"],
  "archived": false,
  "payload": { ... }
}
```

### Entity payloads

- `project`
  - `slug`, `title`, `summary`, `status`
- `task`
  - `slug`, `title`, `summary`, `status`, `priority`, `acceptance`
- `decision`
  - `slug`, `title`, `summary`, `rationale`
- `constraint`
  - `slug`, `title`, `summary`, `impact`
- `status`
  - `slug`, `title`, `summary`, `state`
- `next_step`
  - `slug`, `title`, `summary`, `owner`
- `postmortem`
  - `slug`, `title`, `summary`, `root_cause`

The first implementation should define all entity structs now, even if only a subset gets initial CLI creation paths.

## Storage Strategy

### Phase 1

- Single file: `memoryport/project-records.jsonl`
- Append-only writes
- Read-all then filter in memory
- IDs are deterministic enough for human use:
  - `pm_<timestamp>_<entity>_<slug>`

### Why this shape

- Matches the repo’s current JSONL durability model.
- Avoids premature SQLite or cross-file coordination.
- Keeps project/task records auditable and easy to diff.
- Lets retrieval read one source of truth for structured coordination state.

## Retrieval Preference Model

Structured project records should be searched before unstructured council records whenever Layers decides memory retrieval is relevant.

Ranking preference:

1. `decision`
2. `constraint`
3. `next_step`
4. `status`
5. `task`
6. `project`
7. `postmortem`

Matching should stay simple for v1:

- token overlap on `title`, `summary`, `project`, `task`, and `tags`
- additive weight by entity kind
- compact summary rendered into `MemoryHit`

This keeps project/task state visible to `layers query` without changing the router.

## CLI Surface

### Initial useful commands

- `layers project create --slug ... --title ... [--summary ...] [--status active]`
- `layers project list [--json]`
- `layers task create --project ... --slug ... --title ... [--summary ...] [--status todo] [--priority ...]`
- `layers task list [--project ...] [--status ...] [--json]`

### Reuse existing write path where useful

Extend `layers remember` later so structured coordination records can also be captured through the same explicit-memory workflow:

- `layers remember decision ... --project ... [--task ...]`
- `layers remember constraint ...`
- `layers remember status ...`
- `layers remember next_step ...`
- `layers remember postmortem ...`

That should be phase 2, after the base project/task commands are in place.

## Workflow Packets

The long-term workflow packet model should stay separate from the canonical record log:

- `workflow.plan`
- `workflow.handoff`
- `workflow.postmortem`

Project/task records answer:

- what exists
- what state it is in
- what was decided
- what should happen next

Workflow packets answer:

- what we plan to do now
- what someone else needs to continue
- what we learned after completion/failure

## Clean Fit With Existing Layers Architecture

- `routing.rs` stays unchanged.
- `memory.rs` gains a structured-record tier ahead of semantic and fallback memory.
- `commands.rs` gains project/task commands and lightweight record append/list helpers.
- `types.rs` gains the canonical project/task entity structs.
- `config.rs` adds the canonical project record path.

This keeps the subsystem inside the current Layers composition:

`query -> route -> structured project records -> semantic memory -> fallback memory -> synthesis`

## Smallest Useful Slice To Implement Now

1. Add canonical types for:
   - `ProjectRecord`
   - `Project`
   - `Task`
   - `Decision`
   - `Constraint`
   - `StatusRecord`
   - `NextStep`
   - `Postmortem`
2. Add storage helpers for:
   - append record
   - load records
   - list projects
   - list tasks
   - search structured project records
3. Add initial CLI:
   - `project create`
   - `project list`
   - `task create`
   - `task list`
4. Integrate structured search into `search_memory()` as a first retrieval tier.

This is the smallest slice that is:

- actually useful
- Layers-native
- low enough risk to land without rewriting the current system

## Deferred Work

- record supersession/versioning
- task dependency graphs
- project/task update commands
- structured `remember` support for decision/constraint/status/next_step/postmortem
- workflow packet persistence and rendering
- Memoryport indexing of derived retrieval text from project records
- richer ranking and status rollups

## Acceptance For This Round

- A user can create and list projects and tasks locally through `layers`.
- Structured records live in one canonical JSONL file.
- `layers query` can surface matching structured project/task records through the memory path.
- Existing query and validation flows still build successfully.
