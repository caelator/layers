Phase 1 completed.

Changes made:
- promoted `memoryport/curated-memory.jsonl` to an explicit canonical path in the config layer
- added dedicated curated-memory load/append/search helpers
- changed retrieval to consume curated hits before generic structured records, semantic Memoryport results, and JSONL fallback
- preserved the existing local-first fallback path and import CLI
- added tests covering curated-only search and curated-first retrieval ordering

Canonical curated memory:
- file: `memoryport/curated-memory.jsonl`
- current records: 15
- `cargo run --quiet -- curated import distilled-memory-import.jsonl` returned `imported: 0`, `skipped: 15`, confirming the distilled import was already fully ingested and the backfill path is idempotent

Validation:
- `cargo test -q` passed: 12 tests
- `cargo run --quiet -- validate` passed with `ok: true` and `replacement_ready: true`

Notes:
- Memoryport semantic retrieval is still optional and currently blocked in this sandbox because the local Ollama embed endpoint is not reachable here; validate still passes because curated canonical retrieval and local fallback remain available
- GitNexus impact analysis reported `HIGH` risk for `search_memory` and `search_project_records`, and `CRITICAL` risk for `load_project_records`; the implementation kept `load_project_records` stable and added narrower curated helpers around it
