# Layers v2 Architecture

*5-year horizon. Ship what matters now. Don't block what matters later.*

---

## 1. Architectural Principles

**Local-first, Rust-first.** No cloud dependencies. No runtime that isn't on the developer's machine. External tools are subprocesses today and may become library calls later, but the execution model stays local.

**Explicit over autonomous.** Every retrieval, routing decision, and writeback is auditable and user-initiated. Layers does not act on its own. It retrieves, synthesizes, and explains — the human decides.

**Stable core, swappable edges.** The artifact types, memory schema, and synthesis pipeline are the durable center. Providers (Memoryport, GitNexus, future tools) are replaceable edges behind clean interfaces. The core should change slowly; providers should be easy to add, swap, or remove.

**Composability through data, not inheritance.** Modules communicate via well-typed structs serializable to JSON. No trait hierarchies for their own sake. A new provider or workflow composes by producing or consuming the same artifact types.

**Refuse by default.** The router's job is to say "no" unless evidence says otherwise. Speculative retrieval pollutes context. Silence is better than noise.

**Audit everything, store little.** Every query produces an audit record. But durable memory is curated — only plans, learnings, and traces that a human explicitly writes back. No automatic memory accumulation.

---

## 2. Stability Tiers

### Stable for years (change rarely, version carefully)

| Element | Why |
|---------|-----|
| Artifact types (`Plan`, `Learning`, `Trace`, `Decision`) | Everything reads and writes these. Changing them breaks all stored records and all consumers. |
| Memory record schema (frontmatter + body) | Stored on disk, read across sessions. Schema changes require migration. |
| Provider trait interface | Every backend implements this. Changing it forces all providers to update simultaneously. |
| CLI command surface (`query`, `remember`, `refresh`, `validate`) | Users and scripts depend on these. |
| Audit record format | Append-only log. Old records must remain parseable. |

### Evolves at medium pace (quarterly)

| Element | Why |
|---------|-----|
| Routing pattern tables and decision tree | Tuned as usage patterns emerge. Internal to the router — no external contract. |
| Synthesis templates and word limits | Presentation layer. Adjust based on what consumers (LLMs, humans) actually need. |
| Ranking heuristics (token overlap, kind weights, artifact bonuses) | Internal scoring. Improve as better signals are discovered. |

### Evolves quickly (weekly/monthly)

| Element | Why |
|---------|-----|
| Provider implementations (Memoryport subprocess wrapper, GitNexus CLI calls) | Backend details. Change whenever the upstream tool changes. |
| Output formatting (context_text rendering) | Cosmetic. Iterate freely. |
| Validation test cases | Grow as the system grows. |

---

## 3. Artifact Type System

Four artifact types. Each has a fixed `kind` discriminator, a creation timestamp, and a body. Extensions go in metadata — the core schema does not grow.

```rust
/// The four durable artifact types in Layers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Plan,
    Learning,
    Trace,
    Decision,
}
```

### Plan
A task-scoped intention: what will be done, why, and what constraints apply. Plans reference a task name and may include a full markdown body (from a file) and an optional artifacts directory.

### Learning
A distilled insight or constraint discovered during work. Short, declarative, reusable. Learnings are the highest-value memory type — they compress experience into guidance.

### Trace
A record of what happened during execution. Traces are lower-value individually but valuable in aggregate for pattern detection. They capture task context, outcomes, and timing.

### Decision
A resolved choice with rationale. Decisions are the output of deliberation — they record what was chosen, what was rejected, and why. This type is new in v2; in v1, decisions were embedded inside plans or learnings. Elevating them to a first-class type makes them directly queryable.

### On-disk format

All artifacts are stored as JSONL (one JSON object per line). Each record contains:

```json
{
  "kind": "learning",
  "timestamp": "2026-03-31T14:22:00Z",
  "task": "auth-middleware-rewrite",
  "task_type": "architecture",
  "summary": "Session tokens must not be stored in middleware state — legal/compliance requirement",
  "body": null,
  "artifacts_dir": null,
  "metadata": {}
}
```

The `metadata` field is an open map for provider-specific or workflow-specific data. The core never inspects it. This is the extension point — not new top-level fields.

### Versioning

Records carry no explicit version field. Instead, the schema is governed by two rules:
1. **Additive only.** New optional fields may be added. Required fields are never removed or renamed.
2. **Unknown fields are preserved.** Deserialization ignores unknown keys; serialization round-trips them. This lets newer writers coexist with older readers.

If a breaking change is ever needed (it shouldn't be), introduce a new filename (e.g., `council-plans-v2.jsonl`) rather than migrating in place.

---

## 4. Provider Interfaces

A provider is anything that answers a structured query and returns ranked results. Today there are two: Memoryport (semantic memory) and GitNexus (code graph). The interface must be narrow enough that adding a third provider (e.g., a local vector store, a documentation index, a test-result database) requires no changes to routing, synthesis, or the CLI.

```rust
/// A single result from a provider query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHit {
    /// Human-readable label (e.g., "Definition: validate_user [fn] in src/auth.rs")
    pub summary: String,
    /// Relevance score, provider-defined scale normalized to 0.0..1.0
    pub score: f64,
    /// Which provider produced this hit
    pub source: String,
    /// Provider-specific structured data (kind, path, line number, etc.)
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

/// A provider's response to a query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResult {
    pub hits: Vec<ProviderHit>,
    /// If the provider encountered a non-fatal issue (e.g., not indexed, tool missing)
    pub issue: Option<String>,
}

/// The provider contract.
pub trait Provider {
    /// Human-readable name (e.g., "memoryport", "gitnexus")
    fn name(&self) -> &str;

    /// Whether this provider is available and ready.
    fn is_available(&self) -> bool;

    /// Execute a query and return ranked results.
    fn query(&self, query: &str, limit: usize) -> anyhow::Result<ProviderResult>;

    /// Optional: re-index or refresh the provider's backing store.
    fn refresh(&self) -> anyhow::Result<()> {
        Ok(()) // default: no-op
    }
}
```

### Why a trait now (not later)

The current code already has two providers with nearly identical call patterns: spawn subprocess, parse output, normalize results, handle errors. The trait extracts a pattern that already exists. It does not introduce speculative abstraction — it names a boundary that the code already respects informally.

### Provider registration

Providers are instantiated at startup and collected into a `Vec<Box<dyn Provider>>`. No dynamic plugin loading. No registry. Adding a provider means writing a struct that implements `Provider` and adding one line to the startup sequence. This is the Lego model: snap a new piece on, don't redesign the baseplate.

```rust
fn providers(config: &Config) -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(MemoryportProvider::new(config)),
        Box::new(GitNexusProvider::new(config)),
    ]
}
```

### Score normalization

Each provider returns scores on its own scale. The synthesis layer normalizes to 0.0..1.0 using provider-specific logic (e.g., Memoryport cosine similarity is already 0..1; GitNexus relevance scores may need clamping). Normalization lives in the provider impl, not in synthesis.

---

## 5. Curated Memory Schema

Memory in Layers is curated, not accumulated. Records enter the system only through explicit `remember` commands. This is a deliberate constraint — automatic memory leads to noise, staleness, and unbounded growth.

### Schema

Every memory record is an artifact (see Section 3) stored in a council JSONL file. The four artifact kinds map to three storage files:

| Kind | File | Typical volume |
|------|------|----------------|
| `plan` | `memoryport/council-plans.jsonl` | 10s per project |
| `learning` | `memoryport/council-learnings.jsonl` | 10s–100s per project |
| `trace` | `memoryport/council-traces.jsonl` | 100s per project |
| `decision` | `memoryport/council-decisions.jsonl` | 10s per project |

### Retrieval hierarchy

1. **Semantic search** (Memoryport/uc) — when available, this is the primary retrieval path. Returns hits ranked by embedding similarity.
2. **Token-overlap fallback** — when Memoryport is unavailable, Layers loads council JSONL files and ranks by token intersection with the query. This ensures Layers always works, even without Memoryport installed.

### Memory ranking signals (stable)

| Signal | Weight | Rationale |
|--------|--------|-----------|
| Semantic similarity (from Memoryport) | Primary | Best signal when available |
| Token overlap with query | Fallback primary | Crude but reliable |
| Artifact kind (`learning` > `plan` > `decision` > `trace`) | Tiebreaker | Learnings are highest-value; traces are lowest |
| Recency (newer > older) | Secondary | Recent context is more likely relevant |
| Decision signals (pattern-matched keywords) | Bonus | Records containing explicit decisions/constraints are more actionable |

### Memory lifecycle

Records are append-only. There is no update or delete operation in v2. If a learning becomes stale, the user writes a new learning that supersedes it — the ranking system naturally surfaces the newer record. A future `forget` or `archive` command could be added without changing the schema.

### Memory cap

To prevent unbounded growth from degrading retrieval quality, Layers enforces soft caps:
- Semantic search: top-k is caller-specified (default 8)
- Fallback search: same limit
- Synthesis: hard cap of 6 evidence items in the brief

No per-file size limits are enforced in v2. If a council file grows beyond ~10K records, retrieval quality will degrade naturally (more noise in fallback search). This is acceptable for the 5-year horizon — most projects won't hit it. If they do, a future compaction pass (deduplicate, archive old traces) can be added without schema changes.

---

## 6. Workflow Model

A workflow is a sequence of Layers operations that together accomplish a developer task. Workflows are not enforced by the system — they are patterns documented and encouraged. Layers provides the primitives; workflows compose them.

### Workflow 1: Planning

**When:** Starting a new task, feature, or investigation.

```
1. layers query "what do we know about <topic>"
   → retrieves relevant memory + graph context
   → developer reads synthesis, identifies gaps

2. Developer writes a plan (markdown file)

3. layers remember plan --task "task-name" --file plan.md
   → plan is stored as a curated memory record
   → available for future queries
```

**What Layers provides:** Context retrieval before planning. Durable plan storage after planning. The planning itself is the developer's job.

### Workflow 2: Handoff

**When:** Passing work to another developer, to a future session, or to an AI agent.

```
1. layers query "current state of <task>" --json
   → retrieves all memory related to the task
   → structured output suitable for piping to another tool

2. Developer reviews the synthesis
   → decides what to include in the handoff

3. layers remember learning --summary "key constraint or decision"
   → captures the non-obvious parts that aren't in the code

4. layers remember trace --task "task-name" --summary "what happened"
   → records execution history for the recipient
```

**What Layers provides:** A retrievable history of what was decided and why. The handoff document itself may be external (PR description, Slack message, etc.) — Layers supplies the evidence that informs it.

### Workflow 3: Postmortem

**When:** After an incident, failed approach, or completed milestone.

```
1. layers query "what went wrong with <topic>"
   → retrieves related plans, traces, decisions

2. Developer identifies lessons

3. layers remember learning --summary "lesson learned"
   → distilled insight, queryable in future sessions

4. layers remember decision --task "topic" --summary "we chose X over Y because Z"
   → captures the resolved choice for future reference
```

**What Layers provides:** Retrieval of prior context to inform the postmortem. Durable storage of lessons and decisions so they are available when similar situations arise.

### Optional Workflow: Review

**When:** Reviewing a PR or design document.

```
1. layers query "impact of changes to <module>"
   → graph provider returns structural dependencies
   → memory provider returns prior decisions about the module

2. Developer uses the synthesis to inform review comments
```

**What Layers provides:** Structural and historical context that a reviewer wouldn't otherwise have immediately available.

---

## 7. Module Contracts

The v1 refactor produced 9 modules. V2 preserves the module boundaries but sharpens the contracts.

### Module dependency graph

```
main.rs
  └── commands.rs
        ├── routing.rs      (pure: query → RouteDecision)
        ├── providers/
        │     ├── mod.rs     (Provider trait, ProviderHit, ProviderResult)
        │     ├── memory.rs  (MemoryportProvider impl)
        │     └── graph.rs   (GitNexusProvider impl)
        ├── synthesis.rs     (RouteDecision + ProviderResults → context payload)
        └── memory_ranking.rs (ranking, signal extraction, brief synthesis)
  types.rs    (ArtifactKind, shared structs — depended on by all)
  config.rs   (paths, env — depended on by providers and commands)
  util.rs     (pure helpers — depended on by everyone, depends on nothing)
```

### Contract per module

| Module | Input | Output | Side effects |
|--------|-------|--------|-------------|
| `types` | — | Type definitions | None |
| `config` | Environment, filesystem | Paths | None (reads env/fs but does not mutate) |
| `util` | Primitives | Primitives, JSON values | `append_jsonl` writes to disk; `run_command` spawns subprocesses |
| `routing` | Query string | `RouteDecision` | None (pure function) |
| `providers::memory` | Query string, limit, config | `ProviderResult` | Spawns `uc` subprocess |
| `providers::graph` | Query string, limit, config | `ProviderResult` | Spawns `gitnexus` subprocess |
| `memory_ranking` | Query, hits | Ranked hits, decision signals, brief | None (pure) |
| `synthesis` | RouteDecision, ProviderResults | JSON context payload | None (pure) |
| `commands` | CLI args | JSON/text output | Writes audit log, writes council JSONL |

### Key invariant

**Providers never call each other.** Memory doesn't know about graph. Graph doesn't know about memory. Only `commands.rs` orchestrates them together through the `Provider` trait. This is what makes adding a third provider trivial.

---

## 8. Migration Plan

Migration from v1 to v2 is incremental. No big-bang rewrite. Each step is independently shippable and testable.

### Phase 1: Extract the Provider trait (week 1)

1. Create `src/providers/mod.rs` with the `Provider` trait, `ProviderHit`, and `ProviderResult` types.
2. Create `src/providers/memory.rs` — move `search_memory_semantic`, `search_memory_fallback`, and `search_memory` into a `MemoryportProvider` struct implementing `Provider`. The internal ranking/synthesis helpers move to `src/memory_ranking.rs`.
3. Create `src/providers/graph.rs` — move `gitnexus_indexed`, `query_graph`, `normalize_graph_output` into a `GitNexusProvider` struct implementing `Provider`.
4. Update `commands.rs` to instantiate providers and call them through the trait.
5. Delete old `src/memory.rs` and `src/graph.rs`.
6. `cargo test` + `cargo run -- validate` must pass.

**Risk:** The memory module currently mixes retrieval (provider concern) with ranking and synthesis (consumer concern). The split must be clean — ranking stays in `memory_ranking.rs`, not in the provider.

### Phase 2: Introduce ArtifactKind and Decision type (week 2)

1. Add `ArtifactKind` enum to `types.rs`.
2. Add `decision` as a valid kind in `remember` command.
3. Create `memoryport/council-decisions.jsonl`.
4. Update `config::council_files()` to include the new file.
5. Update memory fallback search to load decisions.
6. Existing JSONL records remain valid (they already have a `kind` field as a string).

**Risk:** None. Additive change. Old records parse fine.

### Phase 3: Unify MemoryHit into ProviderHit (week 2-3)

1. Replace `MemoryHit` with `ProviderHit` throughout.
2. Memory-specific fields (`task`, `artifacts_dir`, `kind`) move into `ProviderHit::metadata`.
3. Update ranking and synthesis to read from metadata.
4. Remove `MemoryHit` from `types.rs`.

**Risk:** Medium. Many functions reference `MemoryHit` fields directly. Careful refactor needed. Run validation after each file change.

### Phase 4: Add metadata field to JSONL records (week 3)

1. Add `metadata: {}` to new records written by `remember`.
2. Deserialization already tolerates missing fields (serde default), so old records parse fine.
3. No migration of existing records needed.

### Phase 5: Tests and cleanup (week 4)

1. Add unit tests for each provider (mock subprocess output).
2. Add unit tests for ranking and synthesis.
3. Add integration test with fixture council files (deterministic, no live Memoryport needed).
4. Remove codex-*.md task/review files if desired (they've served their purpose).

---

## 9. Anti-Goals: What Not to Build Yet

| Temptation | Why not |
|------------|---------|
| Dynamic plugin loading (dlopen, WASM, etc.) | Two providers don't justify a plugin system. When there are five, revisit. |
| Automatic memory ingestion | Leads to noise. Curation is the feature, not a limitation. |
| Network-aware providers (cloud APIs, remote vector DBs) | Violates local-first. If needed later, it fits behind the Provider trait without core changes. |
| A query language or filter DSL | Natural language queries routed by pattern matching are sufficient. A DSL adds learning cost for marginal precision. |
| GUI or TUI | CLI + JSON output covers all current consumers (shell scripts, AI agents, editors). |
| Multi-repo support | One workspace, one Layers instance. Multi-repo coordination belongs in the caller. |
| Embedding generation inside Layers | Memoryport owns embeddings. Layers is a router, not an embedding engine. |
| Event-driven / watch-mode architecture | Layers is request-response. No daemon, no file watchers, no background processes. |
| Backwards-compatible shims for v1 internals | No external consumers depend on internal types. Clean break is fine. |

---

## 10. Risks and Mitigations

### Risk: Memoryport or GitNexus upstream breaks

**Likelihood:** Medium (both are actively developed).
**Impact:** Provider returns errors; synthesis degrades.
**Mitigation:** The fallback path (token-overlap for memory, none for graph) ensures Layers never hard-fails. Provider output parsing is lenient — unknown fields are ignored, malformed output returns an empty result with an issue message. Pin to known-good CLI versions in documentation.

### Risk: Council JSONL files grow too large

**Likelihood:** Low (curated memory grows slowly).
**Impact:** Fallback search slows; disk usage grows.
**Mitigation:** Soft limits in documentation. Future compaction command (`layers gc`) can archive old traces without schema changes. Semantic search (Memoryport) scales independently of file size.

### Risk: Provider trait is too narrow or too wide

**Likelihood:** Medium (two providers may not reveal the right abstraction).
**Impact:** Third provider doesn't fit; trait needs breaking change.
**Mitigation:** The trait is minimal (`name`, `is_available`, `query`, `refresh`). It's easier to widen a narrow trait than to narrow a wide one. The `metadata` field in `ProviderHit` absorbs provider-specific data without trait changes.

### Risk: Routing heuristics don't generalize

**Likelihood:** Medium (pattern tables are hand-tuned).
**Impact:** Queries route incorrectly; wrong context is retrieved or useful context is missed.
**Mitigation:** Routing is isolated in one module with no external dependencies. It can be rewritten (e.g., to use a small classifier, TF-IDF, or even an LLM call) without affecting anything else. The audit log provides ground truth for evaluating routing quality.

### Risk: Over-engineering during migration

**Likelihood:** High (architecture documents create pressure to build everything at once).
**Impact:** Weeks spent on abstractions instead of shipping.
**Mitigation:** The phased migration plan (Section 8) is ordered by value and independence. Each phase ships alone. If Phase 1 (Provider trait) is the only thing that lands, v2 is still a meaningful improvement over v1.

---

## 11. Modularity Without Over-Abstraction

The Lego principle: pieces snap together through shared shapes (types), not through a universal connector system.

### What makes it modular

1. **Shared artifact types.** Every module that reads or writes memory uses the same `ArtifactKind` and record schema. Adding a new artifact kind is a one-line enum variant — no new files, no new traits.

2. **Provider trait with metadata escape hatch.** The trait is four methods. Provider-specific richness goes in `ProviderHit::metadata`, not in trait method signatures. A new provider implements four methods and is done.

3. **Pure routing.** The router is a function: `&str → RouteDecision`. It doesn't know about providers, memory, or the graph. Changing routing logic touches one file.

4. **Synthesis as assembly.** Synthesis takes typed inputs (RouteDecision, Vec<ProviderResult>) and produces a JSON payload. It doesn't fetch anything. It doesn't decide anything. It assembles.

5. **Commands as orchestration.** Only `commands.rs` knows the full pipeline. Only it sequences routing → provider queries → synthesis → audit. Everything else is a leaf.

### What keeps it from over-abstracting

1. **No generics where concrete types suffice.** `ArtifactKind` is an enum, not a trait. `RouteDecision` is a struct, not a generic. Types are concrete until proven otherwise.

2. **No registry pattern.** Providers are a `Vec` constructed at startup, not a global registry with string-key lookups. Adding a provider is a code change, not a config change. This is correct for 2-5 providers.

3. **No middleware or interceptor chains.** The pipeline is a linear sequence in `commands.rs`. If a step needs to be added, add it to the sequence. Middleware patterns are justified at 10+ cross-cutting concerns, not 2.

4. **No config-driven behavior.** Layers has paths and environment variables, not a configuration DSL. Behavior changes are code changes. This is correct for a tool with one primary user (the developer) and one deployment target (their laptop).

5. **Extension by addition, not modification.** New artifact kind → add enum variant. New provider → add struct + impl. New workflow → add command. Nothing existing changes. This is the goal. When adding something new requires modifying something old, that's a design smell worth investigating.

---

## Appendix: Decision Record

| Decision | Chosen | Rejected | Why |
|----------|--------|----------|-----|
| Provider abstraction | Trait | Module-level functions (status quo) | Two providers with identical patterns justify the trait. Three would make it obvious, but waiting costs more than the trait does. |
| Artifact storage | JSONL (status quo) | SQLite, custom binary | JSONL is append-friendly, human-readable, diffable, and trivially parseable. SQLite adds a dependency for no current benefit. |
| Decision as first-class type | New `Decision` kind | Embedded in plans/learnings | Decisions are the most queryable memory type. Elevating them makes them directly retrievable. |
| Memory curation | Explicit `remember` only | Automatic ingestion | Curation is the moat. Automatic memory becomes noise. |
| Score normalization | Provider-side (0..1) | Consumer-side | Each provider knows its own scale. Normalization at the boundary is cleaner than normalization at consumption. |
| Migration strategy | Incremental phases | Big-bang rewrite | Each phase ships independently. Reduces risk. Allows course correction. |
| Plugin system | Not yet | Dynamic loading | Two providers. The third one will tell us if we need plugins. Probably not. |
