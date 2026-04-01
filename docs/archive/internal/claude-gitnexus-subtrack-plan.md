# GitNexus Leverage Subtrack — Plan

## 1. Target End-State

GitNexus becomes a first-class workflow participant in Layers, not just a query backend. Concretely:

- **Graph artifacts** are produced during planning, review, and handoff — structured outputs that capture code-structural context (blast radius, affected flows, dependency clusters) alongside the human intent already stored in council records.
- **Council workflows consume graph artifacts directly** — a plan record can reference the impact scope it was written against; a review can include the detected-changes diff; a handoff can carry the subgraph of code touched.
- **Routing uses graph signals** — `route_query` considers whether the query touches symbols/flows already referenced in recent council artifacts, biasing toward graph when structural context exists.

What this is NOT: a general-purpose code analysis platform. GitNexus stays scoped to making Layers' existing council + continuity workflows structurally aware.

## 2. Graph Artifact Types to Introduce

Four new artifact shapes, stored as typed metadata within existing council JSONL records (no new file formats):

| Artifact | Stored In | Key Fields | Produced By |
|----------|-----------|------------|-------------|
| **ImpactSnapshot** | council-plans.jsonl | `target_symbols`, `blast_radius` (d1/d2/d3 counts), `risk_level`, `affected_processes` | `layers plan --with-impact` |
| **ChangeScope** | council-traces.jsonl | `changed_symbols`, `changed_files`, `affected_flows`, `unexpected_deltas` | `layers remember trace --with-scope` |
| **DependencySlice** | council-plans.jsonl | `entry_points`, `subgraph_edges`, `cluster_id` | `layers plan --with-deps` |
| **ReviewDiff** | council-decisions.jsonl | `before_scope`, `after_scope`, `drift_symbols` (symbols changed but not in original plan) | `layers review` (new command) |

Design rules:
- Each artifact is a JSON object in the record's `metadata` field — no separate files.
- All artifacts include `gitnexus_index_version` (from `.gitnexus/meta.json`) so staleness is detectable.
- Artifacts are optional enrichments — council records work without them.

## 3. Which Workflows Consume Artifacts First

Priority order based on immediate value:

### A. Planning (highest value)
**Before**: Developer writes a plan, `layers remember plan` stores it. No structural awareness.
**After**: `layers plan --task "X" --targets "fn_a,fn_b" --file plan.md` automatically runs `gitnexus_impact` on each target, attaches an ImpactSnapshot, and stores the enriched plan. The developer sees blast radius alongside their intent.

### B. Pre-Commit Review
**Before**: Developer runs `gitnexus_detect_changes` manually (or forgets).
**After**: `layers review` runs detect-changes, compares against the most recent plan's ImpactSnapshot, and flags drift — symbols that changed but weren't in the planned scope. Produces a ReviewDiff artifact stored as a decision record.

### C. Handoff
**Before**: Handoff is a `layers query` plus manual `remember learning`.
**After**: `layers handoff --task "X"` gathers the plan's ImpactSnapshot, any traces with ChangeScope, and the current detect-changes output into a single structured handoff payload. The next developer (or session) gets both human context and structural context.

### D. Postmortem (deferred — see section 6)

## 4. Integration Points with Council and Implementation Flows

### 4a. Routing (`routing.rs`)
- Add a `graph_context_available` signal: when the query mentions symbols that appear in recent council artifacts' `target_symbols` or `changed_symbols`, boost the graph route score.
- Keep the signal additive — it biases toward `"both"` but never overrides explicit routing.

### 4b. Synthesis (`synthesis.rs`)
- When both memory and graph results exist, and the memory results include ImpactSnapshot metadata, synthesize a "structural context" section that summarizes blast radius and affected flows in the output.
- Template: `"Structural context: {target} has {d1_count} direct dependents, participates in {flow_names}. Risk: {level}."`

### 4c. Memory Ranking (`memory_ranking.rs`)
- Records with graph artifacts get a small ranking boost when the query contains structural keywords (function names, "impact", "breaks", "callers").
- Records whose `gitnexus_index_version` is stale get a ranking penalty (their structural claims may be outdated).

### 4d. Commands (`commands.rs`)
- `handle_plan()` — new handler wrapping `handle_remember` for plan kind, with optional `--targets` flag that triggers impact snapshot generation via `graph.rs`.
- `handle_review()` — new handler: runs detect-changes, loads most recent plan, computes drift, stores ReviewDiff.
- `handle_handoff()` — new handler: aggregates plan + traces + current scope into a structured output.

### 4e. Graph (`graph.rs`)
- Add `impact_snapshot(targets: &[String]) -> ImpactSnapshot` — wraps `gitnexus_impact` for each target, aggregates into the artifact shape.
- Add `detect_scope() -> ChangeScope` — wraps `gitnexus_detect_changes`, parses into artifact shape.
- Both return structured Rust types, not raw JSON strings.

## 5. Phased Execution Order

### Phase 1: Graph Artifact Types + Impact in Planning
**Scope**: Define artifact structs in `types.rs`. Implement `impact_snapshot()` in `graph.rs`. Wire `handle_plan()` in `commands.rs`. Add `plan` subcommand to CLI.
**Validates**: That enriched plans are useful and the artifact shape is right.
**Exit criterion**: `layers plan --task "X" --targets "fn_a" --file plan.md` stores a plan with ImpactSnapshot metadata. `layers query` retrieves it with structural context in synthesis output.

### Phase 2: ChangeScope + Review Command
**Scope**: Implement `detect_scope()` in `graph.rs`. Define ReviewDiff. Wire `handle_review()` in `commands.rs`. Add drift detection (compare ChangeScope against most recent plan's ImpactSnapshot).
**Validates**: That pre-commit review catches scope drift.
**Exit criterion**: `layers review` flags symbols changed outside the planned scope.

### Phase 3: Handoff Command
**Scope**: Wire `handle_handoff()` — aggregation logic over existing artifacts. Output as structured JSON or markdown.
**Validates**: That the accumulated artifacts compose into useful handoff context.
**Exit criterion**: `layers handoff --task "X"` produces a payload combining plan intent, structural scope, and change history.

### Phase 4: Routing + Ranking Integration
**Scope**: Add `graph_context_available` signal to `routing.rs`. Add staleness penalty to `memory_ranking.rs`. Add structural-context section to `synthesis.rs`.
**Validates**: That graph-aware routing improves query relevance.
**Exit criterion**: Queries about symbols referenced in recent plans route to graph more reliably. Stale artifacts rank lower.

## 6. What to Defer

- **Postmortem workflow**: Useful but lower priority. The pattern (query + learning) already works; enriching it with graph artifacts can wait until the plan/review/handoff loop is proven.
- **Automatic re-indexing triggers**: The PostToolUse hook already handles this. No need for Layers to own index freshness.
- **Graph-based query expansion**: Using the call graph to expand a query to related symbols is interesting but speculative. Defer until routing integration (Phase 4) is stable.
- **Visualization / TUI**: Structured JSON output is sufficient. No rendering layer.
- **Cross-repo graph artifacts**: Out of scope. Layers is single-repo.
- **Embedding-aware artifact search**: Memoryport already handles semantic search. Don't duplicate it with graph embeddings inside Layers.

## 7. Definition of Done

This subtrack is complete when:

1. **Four artifact types** (ImpactSnapshot, ChangeScope, DependencySlice, ReviewDiff) are defined as Rust structs in `types.rs` and serializable to/from council JSONL metadata.
2. **`layers plan`** stores enriched plans with impact snapshots when `--targets` is provided.
3. **`layers review`** detects and reports scope drift between planned and actual changes.
4. **`layers handoff`** produces a composite structural+intent payload for a given task.
5. **Routing** biases toward graph when queries reference symbols in recent graph artifacts.
6. **Synthesis** includes a structural-context section when graph artifacts are present.
7. **All existing tests pass** — no regressions in current query/remember/refresh/validate flows.
8. **`layers validate`** checks graph artifact round-tripping (serialize → store → retrieve → deserialize).
