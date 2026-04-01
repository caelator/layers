# Layers Core Closure Summary

The remaining Layers core work is not a broad architecture exercise. It is a closure problem: finish canonical curated memory, make ingestion/backfill operational, tighten Memoryport and GitNexus as typed first-class providers, support the small council workflow set, and harden the Rust implementation enough to replace the older track cleanly.

## Recommended Priority

1. Make curated memory the single canonical durable record system.
2. Finish ingestion and backfill so canonical memory can be populated repeatably.
3. Tighten Memoryport integration around that canonical record model.
4. Keep GitNexus operational for code-understanding workflows, with stable normalization and tests.
5. Close the council workflow surface around plan, handoff, postmortem, and durable promotion of lessons.
6. Use tests and deterministic validation as the final replacement gate.

## Main Risks

- dual-path storage between canonical and legacy records will keep retrieval behavior ambiguous
- ingest/backfill can remain a partial script path instead of a maintained product path
- provider logic can stay entangled with local storage and machine-specific validation behavior
- council support can drift into PM/platform sprawl if the workflow surface is not held narrow
- relying on `validate` without deeper tests will leave ranking and normalization regressions under-covered

## Done vs Deferred

Done means:

- `memoryport/curated-memory.jsonl` is the canonical curated store
- durable writes and promotions land there through explicit supported paths
- backfill is idempotent and auditable
- Memoryport and GitNexus are wrapped by stable provider boundaries
- council workflows are explicit and small
- Rust behavior is covered by meaningful unit/integration tests and deterministic validation

Deferred means:

- generic PM/task-system expansion
- workflow engines
- plugin/platform abstractions
- replacing Memoryport or GitNexus internally
- richer ranking or artifact expansion beyond what closure requires

## Closure Standard

This track is closed when Layers is a small Rust core that reliably does three things: council continuity, curated Memoryport-backed retrieval, and GitNexus-assisted code understanding. If an item does not materially improve one of those three outcomes, it should stay deferred.
