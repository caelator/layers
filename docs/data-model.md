# Data Model

## Canonical Files

The current public canonical record store is:

- `memoryport/curated-memory.jsonl`

This file is append-friendly JSONL and stores structured records such as:

- `decision`
- `constraint`
- `next_step`
- `postmortem`

Each record uses a common envelope:

```json
{
  "id": "cm_decision_layers_adopt-direct-uc-retrieval",
  "entity": "decision",
  "project": "layers",
  "created_at": "2026-04-01T12:00:00Z",
  "source": "curated-import",
  "tags": [],
  "archived": false,
  "payload": {
    "type": "decision",
    "slug": "adopt-direct-uc-retrieval",
    "title": "Adopt direct uc retrieval",
    "summary": "Layers should use uc semantic retrieval before local token overlap fallback.",
    "rationale": "This preserves semantic recall when MemoryPort is available without making it a hard dependency."
  }
}
```

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

1. MemoryPort semantic retrieval through `uc`
2. local token-overlap search over canonical curated records and workflow JSONL files

When a query is routed to graph, Layers delegates to GitNexus.

## Practical Rule

If you are deciding what should be reviewed, versioned, or curated by hand, prefer:

- `memoryport/curated-memory.jsonl`
- public docs in `docs/`

If you are looking at logs, scratch workflow state, or replay artifacts, treat them as generated local output unless explicitly promoted.
