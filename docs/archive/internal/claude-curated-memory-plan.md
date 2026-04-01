# Curated Memory for Layers — Implementation Plan

## Problem

Layers currently derives decision signals by pattern-matching against free-text JSONL records and Memoryport semantic search results. The `extract_decision_signals` function in `memory.rs` is a brittle keyword scanner — it only surfaces decisions that happen to contain specific substrings. There is no canonical, structured store for the things that actually matter most: decisions made, constraints adopted, project status, and next steps. These are reconstructed on every query from whatever happens to match.

Curated memory records fix this by giving first-class structured storage to high-signal project knowledge, retrieved before (and ranked above) unstructured memory hits.

## Record Schema

Each curated memory record is a single JSON object stored in an append-only JSONL file. All records share a common envelope; the `body` field varies by kind.

```jsonc
{
  // Envelope — same for every record
  "id": "cm_20260331T120000Z_a1b2",   // "cm_" + ISO timestamp + 4-char hex
  "kind": "decision",                   // decision | constraint | status | next_step | postmortem
  "created_at": "2026-03-31T12:00:00Z", // ISO 8601 UTC
  "supersedes": null,                   // optional: id of the record this replaces
  "tags": ["routing", "v1"],            // free-form, used for filtering
  "source": "manual",                   // manual | backfill | agent

  // Body — kind-specific
  "title": "Layers v1 is an explicit tool, hook-ready later",
  "body": "We decided Layers v1 should be invoked as an explicit CLI tool rather than running as an automatic hook...",
  "context": "Discussed during council round 2, codex-refactor-review.md"
}
```

### Kind-specific field guidance

| Kind | `title` | `body` | `context` |
|------|---------|--------|-----------|
| `decision` | What was decided | Why, alternatives rejected | Where/when discussed |
| `constraint` | The constraint | Why it exists, consequences | Origin (legal, perf, user pref) |
| `status` | Current state | Details, blockers | What changed and when |
| `next_step` | What to do next | Acceptance criteria, dependencies | Who requested, priority |
| `postmortem` | What went wrong | Root cause, what was learned | Timeline, affected systems |

All fields are strings. `body` and `context` are optional but encouraged. `tags` is an optional array of strings. `supersedes` is optional and points to the `id` of a record this one replaces (the old record stays in the file for auditability but is excluded from active retrieval).

## Storage Strategy

### File location

```
memoryport/curated.jsonl
```

One file, append-only, same directory as the existing council files. This keeps the storage model simple and consistent with the existing JSONL pattern.

### Why one file, not one-per-kind

- Five tiny files would add config surface for no benefit at current scale.
- Filtering by `kind` is a JSON field check, not a file-system concern.
- If the file grows past ~5000 records, split then. Not now.

### ID generation

`cm_` prefix + ISO 8601 timestamp (compact, no dashes) + `_` + 4 random hex chars. Generated in Rust with `chrono::Utc::now()` and `rand` (or just read 2 bytes from `/dev/urandom` to avoid adding a dep). Example: `cm_20260331T120000Z_a1b2`.

IDs are for `supersedes` references and audit trails, not for database-style lookups.

### Supersession model

When a curated record is updated, a new record is appended with `supersedes` pointing to the old record's `id`. On load, records whose `id` appears in any other record's `supersedes` field are excluded from the active set. The old record remains in the file for auditability.

## Rust Implementation

### New type: `CuratedRecord`

Add to `src/types.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuratedRecord {
    pub id: String,
    pub kind: String,        // decision, constraint, status, next_step, postmortem
    pub created_at: String,  // ISO 8601
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source: String,      // manual, backfill, agent
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub context: String,
}
```

### New module: `src/curated.rs`

Responsibilities:
1. **`load_active_curated()`** — Read `memoryport/curated.jsonl`, parse all records, exclude superseded ones, return `Vec<CuratedRecord>`.
2. **`search_curated(query, limit)`** — Token-match query against `title + body + tags`, rank by overlap + kind weight. Return `Vec<CuratedRecord>`.
3. **`append_curated(record)`** — Validate kind, generate id if missing, append to file.
4. **`generate_curated_id()`** — Produce the `cm_` prefixed id.

Kind weights for ranking:

| Kind | Weight |
|------|--------|
| `decision` | 5 |
| `constraint` | 5 |
| `next_step` | 4 |
| `status` | 3 |
| `postmortem` | 3 |

### Changes to `src/memory.rs`

The `search_memory` function becomes a three-tier retrieval:

```
1. Curated records  (search_curated)     — structured, high-signal
2. Semantic search  (search_memory_semantic) — unstructured, Memoryport
3. Keyword fallback (search_memory_fallback) — unstructured, JSONL token match
```

Curated hits are converted to `MemoryHit` with `kind` set to the curated kind (e.g., `"decision"`), `source` set to `"curated"`, and `summary` set to `"{title}: {body}"` (compacted).

Curated hits are prepended to the merged results and count toward the limit. If 3 curated hits match, they fill most of the default limit (3), leaving room for at most 1–2 unstructured hits. This ensures curated records always surface first.

### Removal of `extract_decision_signals`

The `extract_decision_signals` function and `synthesize_memory_brief` in `memory.rs` currently reconstruct decision/constraint/status/next_step categories by keyword-matching free text. Once curated records exist, this logic is replaced:

- `synthesize_memory_brief` reads the curated records directly and formats them by kind, no keyword scanning needed.
- `extract_decision_signals` is deleted. The curated record's `kind` field replaces the keyword heuristics.

This is the primary payoff: structured records make the brittle pattern-matching unnecessary.

### Changes to `src/synthesis.rs`

The `<layers_context>` output gains a `Curated:` section before the existing `Memory Brief:` section:

```
Evidence:
  - Curated Decisions: ...
  - Curated Constraints: ...
  - Memory Brief: [from unstructured hits, if any]
  - Memory Sources: ...
  - Graph: ...
```

Curated records are formatted as `[kind] title — body` (compacted to fit the 1200-word budget). They appear first in the evidence block.

## CLI Changes

### `layers remember` — extend with curated kinds

```
layers remember decision --title "..." [--body "..."] [--context "..."] [--tags routing,v1]
layers remember constraint --title "..." [--body "..."] [--context "..."] [--tags ...]
layers remember status --title "..." [--body "..."] [--context "..."] [--tags ...]
layers remember next_step --title "..." [--body "..."] [--context "..."] [--tags ...]
layers remember postmortem --title "..." [--body "..."] [--context "..."] [--tags ...]
```

The existing `plan`, `learning`, `trace` kinds continue to work unchanged, writing to their existing JSONL files. The new kinds write to `memoryport/curated.jsonl`.

### `layers remember` — update (supersede) a record

```
layers remember decision --title "..." --supersedes cm_20260331T120000Z_a1b2
```

Appends a new record with the `supersedes` field set. The old record is excluded from active retrieval.

### `layers curated` — list active curated records

```
layers curated [--kind decision] [--tag routing] [--json]
```

Lists active (non-superseded) curated records, optionally filtered by kind or tag. Default output is a compact table; `--json` emits the full records.

### `layers query` — no CLI changes

Curated records are retrieved automatically as the first tier. No new flags needed. The `--json` output will include curated hits in the `evidence.memory` array with `source: "curated"`.

## Retrieval Order and Synthesis Impact

### Before (current)

```
query → route_query → search_memory(semantic + fallback) → extract_decision_signals(keyword scan) → synthesize_memory_brief → build_context
```

Decision signals are reconstructed from free text on every query. If the text doesn't contain the right keywords, the signal is lost.

### After

```
query → route_query → search_curated → search_memory(semantic + fallback) → synthesize_memory_brief(from curated records) → build_context
```

Decision signals come directly from structured records. Unstructured memory fills in additional context. The keyword scanner is gone.

### Routing impact

The router (`routing.rs`) does not change. Curated records are retrieved whenever the route is `memory_only` or `both`, same as unstructured memory. The difference is only in what gets retrieved and how it's synthesized.

### Synthesis budget

Curated records are compact by nature (title + body). They should consume at most ~200 words of the 1200-word synthesis budget, leaving ample room for unstructured memory and graph hits.

## Migration / Backfill

### Source material

The existing decisions are scattered across:
- `extract_decision_signals()` in `memory.rs` — 9 hardcoded decision/constraint/status/next_step strings
- Free-text content in `memoryport/council-plans.jsonl`, `council-learnings.jsonl`, `council-traces.jsonl`
- Planning documents: `claude-refactor-plan.md`, `codex-layers-v2-architecture.md`, etc.

### Backfill approach

1. **Extract from `extract_decision_signals`**: Each of the 9 hardcoded signals becomes a curated record with `source: "backfill"`. This is mechanical — one record per signal, kind inferred from the prefix (`Decision:` → `decision`, `Constraint:` → `constraint`, etc.).

2. **Extract from planning documents**: Read the existing planning markdown files, identify additional decisions/constraints not covered by the hardcoded signals, create curated records with `source: "backfill"` and `context` pointing to the source document.

3. **Do not backfill from raw JSONL**: The council JSONL files contain session traces and plan dumps, not curated knowledge. Attempting to auto-extract structured records from them would reproduce the keyword-matching problem. Leave them as unstructured memory.

### Backfill script

Add a `layers backfill-curated` subcommand that:
1. Reads `extract_decision_signals` output (or a hardcoded seed list)
2. Generates curated records with `source: "backfill"`
3. Appends to `memoryport/curated.jsonl`
4. Reports what was created

This runs once. It's idempotent — if `curated.jsonl` already contains records with matching titles and `source: "backfill"`, skip them.

### Seed records (from existing hardcoded signals)

```jsonc
// decision
{ "title": "Layers v1 is an explicit tool, hook-ready later", "kind": "decision" }
{ "title": "Layers integrates Memoryport for history and GitNexus for structure", "kind": "decision" }
{ "title": "Durable writeback is explicit, not automatic", "kind": "decision" }
{ "title": "Routing is explicit and refusal-biased", "kind": "decision" }

// constraint
{ "title": "Keep Layers local-first", "kind": "constraint" }
{ "title": "Every retrieval path must be auditable", "kind": "constraint" }
{ "title": "No fake autonomy theater", "kind": "constraint" }

// status
{ "title": "Later planning rounds focused on implementation readiness", "kind": "status" }
{ "title": "Validation is a first-class requirement", "kind": "status" }
```

## What Not To Do Yet

1. **No vector embeddings for curated records.** Curated records are small, structured, and few. Token matching is sufficient. Semantic indexing can be added later if the curated set grows past ~500 records.

2. **No curated record editing in-place.** The supersession model (append new, mark old as superseded) is simpler and preserves auditability. No need for mutable updates.

3. **No automatic curation from agent output.** Records are created via `layers remember` (manual or scripted). An agent can call this CLI command, but there is no auto-extraction pipeline that watches conversations and creates curated records. That's a future feature.

4. **No web UI or TUI for browsing curated records.** `layers curated` with `--json` is the interface. Pipe to `jq` if you want filtering.

5. **No cross-repo curated memory.** Records live in the project's `memoryport/` directory. Sharing across repos is out of scope.

6. **No schema versioning.** The schema is simple enough that additive changes (new optional fields) won't break existing records. If a breaking change is ever needed, handle it then.

7. **No TTL or expiry.** Curated records don't auto-expire. Use `supersedes` to replace stale records. Pruning is a human decision.

8. **No dependency on `rand` crate.** Use `/dev/urandom` (2 bytes → 4 hex chars) or `std::collections::hash_map::RandomState` to avoid adding a new dependency for ID generation.

## Implementation Order

1. **Add `CuratedRecord` to `types.rs`** — struct, derive Serialize/Deserialize
2. **Add `src/curated.rs`** — load, search, append, id generation
3. **Extend `handle_remember` in `commands.rs`** — accept new kinds, route to curated storage
4. **Add `layers curated` subcommand** — list active records
5. **Wire curated search into `search_memory`** — prepend curated hits
6. **Update `synthesize_memory_brief`** — read from curated records directly
7. **Update `build_context` in `synthesis.rs`** — add `Curated:` section
8. **Delete `extract_decision_signals`** — no longer needed
9. **Add `layers backfill-curated`** — seed from hardcoded signals
10. **Run backfill** — populate `memoryport/curated.jsonl`
11. **Update `handle_validate`** — add a curated-record smoke test
