# Layers Phase IV: Memoryport & GitNexus Integration Tightening

*Assumes Phase I (curated memory canonical), Phase II (GitNexus workflow artifacts), and Phase III (council workflow) are complete or landed.*

---

## Goal

Harden the two external integration seams — Memoryport and GitNexus — so they are reliable, well-tested, and clearly contracted. After this phase, Layers' dependency on external CLIs is documented, regression-tested, and gracefully degraded. No new features; just making what exists durable.

---

## Scope (and only this)

1. **Memoryport output contract** — pin and test the `uc` CLI output format Layers parses
2. **GitNexus output contract** — pin and test the JSON schema Layers expects from `gitnexus`
3. **Cross-signal ranking** — when memory hits and graph hits reference the same symbol, boost the combined result
4. **Provider health reporting** — `layers validate` reports structured per-provider status
5. **Council artifact continuity** — ensure council-produced records (from Phase III) round-trip cleanly through Memoryport retrieval

---

## Deliverables

### 4.1 Memoryport Output Contract

| Item | Detail |
|------|--------|
| Pin `uc` output format | Document the exact stdout format Layers parses (regex or JSON). If `uc` supports `--format json`, switch to JSON parsing. |
| Regression tests | Add fixture-based tests that feed known `uc` stdout strings through the parser and assert correct `ProviderHit` output. |
| Malformed output handling | Parser returns `ProviderResult { hits: [], issue: Some("...") }` on unexpected format — never panics, never silently drops results. |
| Version awareness | `layers validate` runs `uc --version` (or equivalent) and reports the version. No hard enforcement — just visibility. |

**Files touched:** `src/memory.rs`, tests.

### 4.2 GitNexus Output Contract

| Item | Detail |
|------|--------|
| Pin JSON response schema | Document the expected shape of `gitnexus` query/impact/context JSON output in code comments and test fixtures. |
| Regression tests | Fixture-based tests: feed known JSON through `normalize_graph_output` and related functions, assert correct results. |
| Schema drift tolerance | Unknown fields are ignored (serde default). Missing expected fields produce a clear issue message, not a crash. |
| Version check in validate | `layers validate` runs `gitnexus --version` and reports it. Warn if the version is older than the tested contract. |

**Files touched:** `src/graph.rs`, tests.

### 4.3 Cross-Signal Ranking

| Item | Detail |
|------|--------|
| Symbol overlap detection | When a memory hit's `summary` or `task` field contains a symbol name that also appears in a graph hit, apply a score boost (e.g., +0.15). |
| Implementation | Simple string containment check in the synthesis/ranking layer — no graph queries, no embedding comparisons. |
| Tests | Unit tests with overlapping and non-overlapping hits, asserting rank order changes. |

**Files touched:** `src/memory.rs` or `src/synthesis.rs` (wherever ranking currently lives).

### 4.4 Provider Health in Validate

| Item | Detail |
|------|--------|
| Structured output | `layers validate` outputs per-provider: `{ name, available, version, issue }`. |
| No trait abstraction | Health checks are concrete per-provider functions called from the validate command. Two providers don't need a health-check trait. |
| Degradation clarity | When a provider is unavailable, validate still passes (degraded mode) but clearly states which capabilities are reduced. |

**Files touched:** `src/commands.rs` (validate handler).

### 4.5 Council Artifact Round-Trip

| Item | Detail |
|------|--------|
| Verify council records parse | Records written by Phase III council workflow (`council-plans.jsonl`, `council-learnings.jsonl`, `council-traces.jsonl`, `council-decisions.jsonl`) must load and rank correctly through both curated retrieval and Memoryport semantic retrieval. |
| Metadata preservation | `metadata.graph_context` and any council-specific metadata fields survive the write → load → rank → synthesize pipeline. |
| Test | Integration test: write a council record with graph metadata, retrieve it via `layers query`, confirm the metadata appears in synthesis output. |

**Files touched:** existing test infrastructure, possibly `src/memory.rs`.

---

## What This Phase Does NOT Do

| Excluded | Why |
|----------|-----|
| New CLI commands | Phase IV is tightening, not expanding. |
| Provider trait refactor | Concrete functions are sufficient for two providers. |
| Embedding generation | Memoryport's responsibility. |
| New artifact types | The four types (plan, learning, trace, decision) are stable. |
| Council orchestration changes | That's Phase III's job. Phase IV just ensures the artifacts survive retrieval. |
| Multi-repo or network features | Out of scope permanently. |

---

## Execution Order

```
4.1 Memoryport contract ──┐
                          ├──► 4.3 Cross-signal ranking ──► 4.4 Validate health
4.2 GitNexus contract ────┘                                        │
                                                                   ▼
                                                        4.5 Council round-trip
```

- 4.1 and 4.2 are independent — can be done in parallel.
- 4.3 depends on both contracts being pinned (needs stable parser output to test ranking).
- 4.4 depends on version-check work from 4.1/4.2.
- 4.5 is a final integration check that validates everything works end-to-end.

---

## Definition of Done

Phase IV is **done** when all of the following are true:

- [ ] `uc` output parsing has fixture-based regression tests covering happy path and malformed input
- [ ] GitNexus JSON parsing has fixture-based regression tests covering happy path, missing fields, and unknown fields
- [ ] Cross-signal ranking is implemented with tests proving symbol-overlap boost changes rank order
- [ ] `layers validate` reports per-provider structured health (`name`, `available`, `version`, `issue`)
- [ ] `layers validate` passes in degraded mode (no providers) and full mode (all providers)
- [ ] Council records with `metadata.graph_context` round-trip through write → retrieve → synthesize without data loss
- [ ] `cargo test` passes with all new tests included
- [ ] No new CLI commands or artifact types were introduced
- [ ] All changes are additive — no breaking changes to existing JSONL schemas or CLI surface

---

## Estimated Effort

~3 working days. Most of the work is writing test fixtures and regression tests for parsers that already exist. Cross-signal ranking is a small, isolated change. Validate health reporting is straightforward structured output.

---

## What Comes After

Phase V should be the final closure phase: clippy/fmt clean, error message audit, CLI help text, CLAUDE.md update, and the full definition-of-done gate from the core closure plan. After Phase V, Layers enters maintenance-only mode.
