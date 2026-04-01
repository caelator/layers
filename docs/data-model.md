# Data Model

## Canonical Files

The current public canonical record store is:

- `memoryport/curated-memory.jsonl`

This file is append-friendly JSONL and stores structured records such as:

- `project`
- `task`
- `decision`
- `constraint`
- `status`
- `next_step`
- `postmortem`

Each record uses a common envelope:

```json
{
  "id": "pm_20260401T120000Z_task_release-readiness",
  "entity": "task",
  "project": "layers",
  "task": "release-readiness",
  "created_at": "2026-04-01T12:00:00Z",
  "source": "manual",
  "tags": [],
  "archived": false,
  "payload": {
    "type": "task",
    "slug": "release-readiness",
    "title": "Finish public docs",
    "summary": "Document release setup and caveats",
    "status": "in_progress",
    "priority": "high"
  }
}
```

## Compatibility Read Paths

Layers still reads from this legacy path if it exists:

- `memoryport/project-records.jsonl`

That file is no longer the canonical write target in the current implementation. It is a compatibility input only.

## Generated Local Artifacts

These files are operational artifacts and should usually stay untracked:

- `memoryport/layers-audit.jsonl`
- `memoryport/council-plans.jsonl`
- `memoryport/council-learnings.jsonl`
- `memoryport/council-traces.jsonl`
- `memoryport/council-runs/`
- `.gitnexus/`
- `target/`

These are useful locally, but they are not the public source of truth for project state.

## Council Artifacts

`layers council run` writes a run directory containing:

- `context.txt`
- `context.json`
- per-stage prompts and outputs
- `run.json`
- `convergence.json`

Those artifacts are generated execution evidence for one run. They are not canonical product configuration.

## Query Inputs

When a query is routed to memory, Layers currently searches in this order:

1. canonical curated records
2. structured record search over the same canonical store
3. Memoryport semantic retrieval through `uc`
4. local fallback JSONL workflow files

When a query is routed to graph, Layers delegates to GitNexus.

## Practical Rule

If you are deciding what should be reviewed, versioned, or curated by hand, prefer:

- `memoryport/curated-memory.jsonl`
- public docs in `docs/`

If you are looking at logs, scratch workflow state, or replay artifacts, treat them as generated local output unless explicitly promoted.
