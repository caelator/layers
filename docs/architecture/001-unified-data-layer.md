# Architecture Design Document 001 — Unified Data Layer

**Status:** Draft (council-reviewed through 2 deliberation rounds)
**Date:** 2026-04-06
**Authors:** AI Council (Gemini + Claude + Codex) with Caelator editorial
**Source:** `.council/design-a-data-layer-implementation/`

---

## 1. Goal and Constraints

**Goal:** Design a unified, shared data layer embedded within the `layers` codebase to serve as the single source of truth for all Caelator ecosystem plugins (`council`, `openclaw-pm`, `research-radar`, `evolve`).

**Hard constraints:**
- Must strictly use **LanceDB** as the underlying vector and structured data store. No alternative databases.
- Must support both **semantic (vector) search** and **structured (relational/metadata) queries** natively.
- Plugins must **cease maintaining isolated persistent storage silos** for shared state.
- This phase is **design only** — no Rust implementation or plugin refactoring.

---

## 2. Architectural Decision: Hybrid with Dedicated Embedding Index + File-based Logs

### 2.1 Why Not a Single Collection

A single monolithic `caelator_memory` collection (the "Everything Bagel" approach) was rejected because:
- Sparse schema, weak typing, heavy serialization logic
- Inline vectors complicate re-embedding when the model upgrades
- No separation of concerns between canonical state and audit traces

### 2.2 Final Design: Approach C

**Core principle:** Keep all shared, queryable, cross-plugin state in LanceDB. Keep append-only audit streams on the filesystem.

| Storage tier | Technology | What lives here |
|---|---|---|
| Canonical shared state | LanceDB | entities, relations, artifacts, embeddings, projections |
| Audit / trace logs | Filesystem | append-only temporal traces, event logs |

**Classification Tiebreaker:** If any predicate is uncertain, default to `LanceDB canonical`. The ADD must include at least one borderline worked example per plugin.

**Canonical store location:** One user-global LanceDB store at `~/.caelator/store/*.lance` with mandatory `workspace_id` on every canonical row. Workspace-relative storage is reserved for append-only local audit logs only.

---

## 3. Unified Memory Language — Schema Definition

### 3.1 MemoryHeader (mandatory on every canonical row)

Every entity carries this header:

```
urn:           string  — globally unique identity (see §3.2)
parent_urn:    Option<string>  — hierarchical parent, if any
workspace_id:  string  — isolation boundary
domain:        string  — derived at write time: plugin_or_functional_area
kind:          string  — derived at write time: specific entity type
tags:          Vec<string>
origin_plugin: string  — which plugin wrote this
created_at:    i64     — Unix timestamp
updated_at:    i64
tombstoned:    bool    — soft-delete flag
```

### 3.2 URN Specification

```
urn:urn:caelator:{plugin_or_domain}:{kind}:{id}
```

Examples:
- `urn:caelator:research-radar:document:sha256-abc123`
- `urn:caelator:council:session:sha256-def456`
- `urn:caelator:layers:projection:sha256-ghi789`

Rules:
- URNs are **opaque for identity logic** — treat them as unique keys
- URNs are **parseable for diagnostics** — extract domain/kind for filtering
- Allowed characters: `[a-z0-9\-]` — lowercase only
- Global uniqueness enforced by `(plugin, kind, id)` tuple
- `id` field: use content-addressed hash (SHA-256 of canonical content) where possible

### 3.3 LanceDB Collections

#### `entities` — canonical stateful nodes

| Column | Type | Notes |
|---|---|---|
| `urn` | string | PRIMARY KEY |
| `parent_urn` | string | nullable |
| `workspace_id` | string | indexed |
| `domain` | string | indexed — derived at write time |
| `kind` | string | indexed — derived at write time |
| `tags` | string[] | array |
| `origin_plugin` | string | |
| `body` | string | JSON payload |
| `created_at` | i64 | Unix timestamp |
| `updated_at` | i64 | |
| `tombstoned` | bool | default false |

**Index:** `(workspace_id, domain, kind)` composite index for efficient filtered queries.

#### `relations` — typed edges between URNs

| Column | Type | Notes |
|---|---|---|
| `urn` | string | PRIMARY KEY |
| `subject_urn` | string | indexed — the source entity |
| `predicate` | string | indexed — relationship type |
| `object_urn` | string | indexed — the target entity |
| `workspace_id` | string | |
| `attributes` | string | JSON metadata |
| `created_at` | i64 | |

**Index:** `(subject_urn, predicate)`, `(predicate, object_urn)` for graph traversal.

#### `artifacts` — large content blobs or external-file descriptors

| Column | Type | Notes |
|---|---|---|
| `urn` | string | PRIMARY KEY |
| `parent_urn` | string | which entity this belongs to |
| `workspace_id` | string | |
| `media_type` | string | MIME type |
| `byte_count` | i64 | |
| `storage_path` | string | local path if not inline |
| `inline_data` | bytes | nullable, for small blobs |
| `origin_plugin` | string | |
| `created_at` | i64 | |
| `tombstoned` | bool | |

#### `embeddings_*` — semantic index collections (one per registered space/dimension)

Collection naming: `embeddings_{space_id}_v{schema_version}`

Example: `embeddings_rr_text_v1`, `embeddings_council_session_v1`

| Column | Type | Notes |
|---|---|---|
| `urn` | string | PRIMARY KEY — `(subject_urn, chunk_id)` |
| `subject_urn` | string | indexed — back-reference to entity |
| `chunk_id` | string | which chunk of the entity |
| `chunk_text` | string | the text that was embedded |
| `vector` | float32[] | the embedding vector |
| `space_id` | string | which embedding space / model |
| `embedding_model` | string | model provenance |
| `embedding_dim` | i32 | dimensionality, must match space |
| `created_at` | i64 | |

**Space registry** (stored as a LanceDB table `embedding_spaces`):

| Column | Type | Notes |
|---|---|---|
| `space_id` | string | PRIMARY KEY |
| `embedding_model` | string | e.g. `sentence-transformers/all-MiniLM-L6-v2` |
| `embedding_dim` | i32 | dimensionality |
| `workspace_id` | string | |
| `schema_version` | i32 | |
| `created_at` | i64 | |

**Rule:** One physical collection per `(space_id, schema_version)`. Mixed-dimension collections are not allowed.

#### `projections` — derived, rebuildable read models

| Column | Type | Notes |
|---|---|---|
| `urn` | string | PRIMARY KEY |
| `projection_name` | string | indexed — which projection |
| `subject_urn` | string | indexed — what this is about |
| `version` | i64 | monotonic version |
| `built_from_entity_version` | i64 | which entity version this was built from |
| `payload` | string | JSON |
| `workspace_id` | string | |
| `stale` | bool | computed: true if entity version > built_from version |
| `created_at` | i64 | |

**Staleness contract:** A projection is **stale** if `entity.updated_at > projection.built_from_entity_version`. Reads must either reject stale projections or lazily rebuild them. No canonical business state may live in projections.

---

## 4. Schema Evolution Policy

This is **promoted to V1 scope** (was V2 in initial council draft):

1. **Additive columns only** as the default evolution path
2. Each collection has a **monotonic schema version** stored in the collection metadata
3. `layers_data::migrate()` upgrades stores **in place** with forward-only migrations
4. Plugins declare a **supported schema-version range** rather than assuming HEAD
5. Schema negotiation: plugin asks `layers-data` "I support schema versions [n, m]; what can you give me?" → `layers-data` responds with the best common version
6. **Non-breaking additive change example:** Adding a new optional `metadata` JSON column to `entities` — all existing rows get `null`, new rows populate it, no plugin breaks

---

## 5. Tombstone Lifecycle

1. **Soft delete:** Set `tombstoned = true`, preserve the row
2. **Retention window:** Configurable, default 30 days (defined in `~/.caelator/config.toml`)
3. **Reap preconditions:** No protected audit-log reference within the retention window
4. **Compaction:** Bounded cleanup runs on startup or on demand — deletes rows where `tombstoned = true AND updated_at < (now - retention_days) AND no audit log references the URN`
5. Tombstones **block semantic index queries** by default (filtered out at query planner level)

---

## 6. API Surface Design

### 6.1 Crate Structure

Within `layers/crates/layers-data/src/`:

```
layers-data/
├── lib.rs
├── schema.rs          # MemoryHeader, URN types, domain/kind constants
├── entity.rs          # EntityStore trait + LanceDB implementation
├── relation.rs        # RelationStore trait + LanceDB implementation
├── artifact.rs        # ArtifactStore trait + LanceDB implementation
├── embedding.rs       # SemanticIndex trait + LanceDB implementation
├── projection.rs      # ProjectionStore trait + LanceDB implementation
├── query.rs           # QueryPlanner — hybrid structured + semantic
├── migration.rs       # Schema versioning, migrate()
├── plugin.rs          # PluginContext, plugin_id constants
├── config.rs          # Store location, retention settings
└── error.rs           # Error types
```

### 6.2 Capability Traits

```rust
// Core store trait
pub trait EntityStore {
    fn put(&self, ctx: &PluginContext, entity: &Entity) -> Result<()>;
    fn get(&self, ctx: &PluginContext, urn: &Urn) -> Result<Option<Entity>>;
    fn list(&self, ctx: &PluginContext, filter: &EntityFilter) -> Result<Vec<Entity>>;
    fn tombstone(&self, ctx: &PluginContext, urn: &Urn) -> Result<()>;
}

// Semantic index — one trait, multiple collections behind it
pub trait SemanticIndex {
    fn upsert_embedding(&self, ctx: &PluginContext, embedding: &Embedding) -> Result<()>;
    fn search(&self, ctx: &PluginContext, query: &SemanticQuery) -> Result<Vec<SearchHit>>;
    fn delete_for_subject(&self, ctx: &PluginContext, subject_urn: &Urn) -> Result<()>;
}

// Hybrid query composition
pub struct QueryPlanner {
    entity_store: Arc<dyn EntityStore>,
    semantic_index: Arc<dyn SemanticIndex>,
}

impl QueryPlanner {
    /// Structured filter + semantic vector search, resolved against canonical entities
    pub fn hybrid_search(&self, ctx: &PluginContext, query: &HybridQuery) -> Result<Vec<MemoryNode>>;
}
```

### 6.3 Write Coordination (Crash-Safe Single-Writer)

**Problem:** Multiple plugins writing concurrently must not corrupt state.

**Solution:** Inside `layers-data`, writes are mediated by a **crash-safe lease mechanism**:

- A process-wide lock file at `~/.caelator/store/.write.lock` with a **lease** (not a naive mutex)
- Lease contains: `holder_pid`, `expires_at` (Unix timestamp), `nonce` (random u64)
- On startup or after crash, a process checks: is `expires_at` in the past? If yes, the lease is stale and recovery proceeds
- Recovery: delete the stale lock file, acquire a fresh lease
- **Bulk re-embedding:** Must use **chunked/yielding writes** or **shadow-build plus atomic cutover** — it may not hold the writer lease for the entire rebuild
- Re-embedding flow: write new vectors to `embeddings_*_shadow`, validate all rows, atomically rename to `embeddings_*`

### 6.4 Plugin Binding

```rust
// Plugin initializes once at startup:
let store = layers_data::open(
    global_store_path(),  // ~/.caelator/store/
    PluginContext {
        plugin_id: "research-radar",
        workspace_id: "default",
        capabilities: &["entities", "embeddings"],
    },
)?;

// Entity + embedding written atomically in one transaction:
store.entity().put(ctx, &entity)?;
store.embedding().upsert_embedding(ctx, &embedding)?;
// or use a combined API:
store.put_node(ctx, &memory_node)?;  // entity + embedding upserted together
```

---

## 7. Migration Strategy

### 7.1 Classification Matrix

For each persisted artifact in each plugin, classify as:

| Classification | Rule | Example |
|---|---|---|
| `LanceDB canonical` | Queried cross-plugin, semantically searchable, or stateful | research-radar document embeddings, council session records, openclaw-pm task states |
| `filesystem audit` | Append-only, never queried cross-plugin, not semantically searchable | Event traces, temporal logs, forensic artifacts |

**Tiebreaker:** When in doubt → `LanceDB canonical`.

### 7.2 Per-Plugin Migration

#### research-radar
- **From:** SQLite keyword search index (existing) + local document blobs
- **To:** `entities` + `embeddings_rr_text_v1` + `artifacts`
- **ETL steps:** Export documents as JSON → parse into `Entity` records → compute embeddings via TurboCALM → upsert to LanceDB → delete SQLite index

#### council
- **From:** In-memory session state, filesystem audit logs
- **To:** `entities` (session records) + filesystem append logs (remain on filesystem as audit trail)
- **ETL steps:** On session close, serialize session summary as `Entity` with `domain=council` → upsert to LanceDB

#### openclaw-pm
- **From:** Task state in memory or filesystem JSON blobs
- **To:** `entities` with `domain=openclaw-pm, kind=task` + `relations` for dependencies
- **ETL steps:** Walk task files → convert each to `Entity` → upsert relations

#### evolve
- **From:** TBD (likely agent memory logs)
- **To:** `entities` + `embeddings` (design to accommodate when migration is scoped)

### 7.3 Borderline Classification Examples

**Example 1 (LanceDB):** `openclaw-pm` task comments. Queriable by `domain=openclaw-pm, kind=comment`, searchable semantically ("show me comments about X"), stateful. → `entities` table.

**Example 2 (Filesystem):** `council` raw turn-by-turn deliberation traces. Append-only audit, never queried cross-plugin, not semantically indexed. → filesystem append log, URN referenced but not canonical.

**Example 3 (Borderline → LanceDB by tiebreaker):** `layers` telemetry aggregation summaries. Uncertain if cross-plugin query needed. Tiebreaker fires → `entities` canonical.

---

## 8. Validation Plan

Before implementing, validate these walkthroughs:

### 8.1 Schema Desk-Check
Map existing plugin data structures to canonical schema — confirm all fields fit, note any that don't.

### 8.2 Compatibility Walkthrough (Pseudocode)
```rust
// Plugin declares supported range
let supported = VersionRange { min: 3, max: 5 };
let available = layers_data::schema_version()?;
// Negotiation: best common version = min(max_promised_by_both)
let agreed = min(supported.max, available.max);
if agreed < supported.min { return Err(Incompatible); }
```

### 8.3 Integrity Walkthrough (Pseudocode)
```rust
// Coordinated entity + embedding upsert
let entity = Entity { urn: new_urn(), body: ..., .. };
let embedding = Embedding { subject_urn: entity.urn, vector: embed(&entity.body)?, .. };
layers_data::put_node(ctx, &entity, &embedding)?;  // atomic inside write lease

// Tombstoning: entity gone from all queries, embeddings cleaned up
layers_data::tombstone_node(ctx, &urn)?;
// embeddings with subject_urn == urn are cascade-deleted by query planner
```

### 8.4 Concurrency Walkthrough
```
Plugin A acquires lease → writes → releases
Plugin B waits (or recovers stale lease) → acquires → writes
Dead writer (crash): lease.expires_at is past → next writer deletes lock → acquires fresh lease
```

### 8.5 Re-embedding Walkthrough
```
1. Create new collection: embeddings_rr_text_v2_shadow
2. Iterate entities in chunks of 100, upsert embeddings to shadow
3. Validate all rows written
4. Atomic rename: embeddings_rr_text_v2_shadow → embeddings_rr_text_v2
5. Delete old collection asynchronously (after grace period)
```

---

## 9. Out of Scope (Do Not Build in This Phase)

- Evaluation of alternative databases (SQLite, Qdrant, Chroma, etc.)
- Writing Rust implementation code for the data layer
- Modifying existing plugin source code to consume the new API
- Network-level synchronization or multi-user concurrent access models
- Generic data ingestors for non-Caelator external tools

---

## 10. Open Questions / Risks

| Risk | Status |
|---|---|
| LanceDB Rust API maturity for filtering, joins, per-space collection management | Needs validation |
| Write coordination crash-safe mechanism feasibility | Needs validation |
| Schema migration ergonomics and rollback strategy | Needs validation |
| Embedding space governance (approved space_ids, re-embedding triggers) | Defined in ADD, needs tooling |
| Projection rebuild discipline — ensuring projections stay rebuildable caches | Design rule stated, tooling TBD |

---

## 11. Next Steps

**V1:** Write this ADD to `layers/docs/architecture/001-unified-data-layer.md`, get it reviewed.

**V2:** Implement `layers-data` crate in `layers/crates/layers-data/`. Start with `EntityStore` + schema versioning + migration runner.

**V3:** Migrate `research-radar` first — it's the most concrete existing store and validates the full ETL pipeline.

---

*This document was produced by an AI Council deliberation (2 rounds, Gemini + Claude + Codex) reviewed by Caelator. The council's SIGTERM'd handoff was reconstructed from `build-plan-round2.md`.*
