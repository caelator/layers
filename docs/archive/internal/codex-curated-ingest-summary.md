# Curated Memory Ingest Summary

- Implemented a minimal curated-memory ingestion path in Rust with a canonical store at `memoryport/curated-memory.jsonl`.
- Added `layers curated import <file>` to import distilled JSONL into typed canonical records.
- Kept structured retrieval first-class by continuing to rank canonical records ahead of fallback memory sources.
- Preserved compatibility by reading both the new canonical file and the legacy `memoryport/project-records.jsonl` path if it exists.

## Import Result

- Source file: `distilled-memory-import.jsonl`
- Imported: 15 records
- Skipped as duplicates: 0
- Canonical output: `memoryport/curated-memory.jsonl`

## Validation

- `cargo test`: pass
- `cargo run -- validate`: pass
- End-to-end memory query now returns structured hits from curated memory, with `memory_provider.ok: true` and `replacement_ready: true`.

## Notes

- Imported curated kinds: `decision`, `constraint`, `status`, `next_step`, `postmortem`
- GitNexus MCP impact/detect tools were not exposed in this Codex environment, so impact assessment was done by manual caller tracing instead of tool-driven graph analysis.
