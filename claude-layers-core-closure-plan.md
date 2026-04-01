# Layers Core / Rust Implementation — Closure Plan

## 1. Target End-State

Layers is a **finished, local-first Rust CLI** that does three things well and nothing else:

1. **Curated memory** — canonical structured storage and ranked retrieval of decisions, constraints, status, next steps, and postmortems via `memoryport/curated-memory.jsonl`
2. **Context routing** — pattern-based query classification that assembles memory + code graph context with graceful degradation
3. **Code understanding** — GitNexus integration providing symbol-aware codebase context alongside memory

The end-state binary:
- Passes all unit and integration tests without external services running
- Produces deterministic output for the same input on any prepared machine
- Has a stable CLI surface (`query`, `remember`, `curated import`, `project`, `task`, `validate`)
- Requires zero maintenance unless the external tool contracts (`uc`, `gitnexus`) change

This is a **tool, not a platform**. It graduates its patterns to Triumvirate and then enters maintenance-only mode.

---

## 2. Architectural Principles

| Principle | Implication |
|-----------|-------------|
| **Local-first** | No network calls. Memoryport and GitNexus are local CLIs. Layers never phones home. |
| **Rust-first** | No Python fallback path. The Rust binary is the only implementation. |
| **Narrow scope** | Three concerns only: curated memory, context routing, code understanding. No task management, no agent loops, no plugin systems. |
| **Explicit over implicit** | Memory is curated by humans/councils, never auto-accumulated. Routing prefers refusal ("neither") over speculation. |
| **Graceful degradation** | Every external dependency (Memoryport semantic, GitNexus) is optional. Core retrieval always works via structured records. |
| **Completion over evolution** | Every change must move toward "done". No speculative abstractions, no open-ended extension points. |

---

## 3. Phased Execution Plan

### Phase 1: Fix Blocking Gaps (Days 1-2)

**Goal:** Eliminate the remaining parity issues that prevent the Rust binary from being the sole implementation.

| Deliverable | Detail |
|-------------|--------|
| Fix default workspace targeting | Derive workspace root from current repo (`.git` ancestor walk), not hardcoded `~/.openclaw/workspace` |
| Validate `--task` and `--summary` enforcement | `layers remember trace` must require both flags |
| Verify `refresh --embeddings` passthrough | Confirm GitNexus `analyze --embeddings` flag is forwarded correctly |

**Artifacts:** Code changes in `config.rs`, `commands.rs`. Updated unit tests.

---

### Phase 2: Test Suite (Days 3-5)

**Goal:** Prove all behavior without live external services.

| Deliverable | Detail |
|-------------|--------|
| Routing unit tests | Cover all 7 decision branches + edge cases (mixed signals, empty queries) |
| Memory ranking tests | Verify tier priority (structured > semantic > fallback), dedup, kind weighting |
| Curated import tests | Happy path, duplicate skipping, malformed input rejection |
| Synthesis tests | MemoryBrief bucket limits, architecture summary generation |
| Graph normalization tests | Process, definition, and process_symbol output formatting |
| Integration test harness | Fixture-based: seed `curated-memory.jsonl` + mock GitNexus output, run full `query` pipeline |
| Validate determinism | `layers validate` passes with no external services, reporting degraded-but-healthy status |

**Artifacts:** New `tests/` directory or expanded `#[cfg(test)]` modules. CI-ready (no Ollama/Memoryport dependency).

---

### Phase 3: Curated Memory Completion (Days 6-8)

**Goal:** Make curated memory the complete, canonical source of structured project knowledge.

| Deliverable | Detail |
|-------------|--------|
| Backfill pipeline | `layers curated backfill` command: scans `council-plans.jsonl`, `council-learnings.jsonl`, `council-traces.jsonl` and extracts promotable records into `curated-memory.jsonl` with human confirmation |
| Archive semantics | `layers curated archive <id>` — sets `archived: true` on a record, excluded from retrieval but preserved in file |
| Curated list/search | `layers curated list [--kind decision] [--project X]` — filtered view of canonical store |
| Migration gate | Once backfill is complete, `project-records.jsonl` fallback path can be removed. Add a deprecation warning if it's still being read. |

**Artifacts:** New subcommands in `commands.rs`, updates to `projects.rs` and `memory.rs`.

---

### Phase 4: Memoryport & GitNexus Tightening (Days 9-11)

**Goal:** Clean integration contracts — no fragile regex parsing, clear error reporting.

| Deliverable | Detail |
|-------------|--------|
| Structured Memoryport output | If `uc` supports `--format json`, switch to JSON parsing. If not, document the regex contract and add regression tests for known output formats. |
| GitNexus query contract | Pin expected JSON response schema. Add version check (`gitnexus --version`) to `validate`. |
| Cross-signal ranking | When a memory hit and a graph hit share a symbol name, boost the combined result. Simple string overlap — no graph queries needed. |
| Provider health in validate | `layers validate` reports: provider name, available (bool), last-checked, known issues. No provider-trait abstraction needed — just structured output. |

**Artifacts:** Updates to `memory.rs`, `graph.rs`, `commands.rs` (validate output). New tests for parse contracts.

---

### Phase 5: Council Workflow Hardening (Days 12-13)

**Goal:** Councils can persist and retrieve their artifacts reliably without Layers needing to understand council orchestration.

| Deliverable | Detail |
|-------------|--------|
| Council artifact schema validation | `layers remember plan/learning/trace` validates required fields before writing |
| Council-to-curated promotion | `layers curated promote --source council-plans.jsonl --kind decision` — extracts structured records from council artifacts into curated store |
| Session tagging | Add optional `--session <id>` to `remember` commands. Enables retrieval scoped to a specific council session. |
| Retrieval by session | `layers query --session <id>` filters to artifacts from that session |

**Artifacts:** Updates to `commands.rs`, `memory.rs`, `types.rs`. New fields in JSONL schema (backward-compatible via `Option`).

---

### Phase 6: Polish & Close (Days 14-15)

**Goal:** Final hardening, documentation, and closure gate.

| Deliverable | Detail |
|-------------|--------|
| Clippy + fmt clean | `cargo clippy -- -D warnings` and `cargo fmt --check` pass |
| Error messages audit | Every user-facing error includes: what failed, why, what to do next |
| CLI help text | All commands have `--help` with examples |
| Audit log cleanup | Remove noisy/redundant fields from `layers-audit.jsonl` entries |
| CLAUDE.md update | Reflect final CLI surface and integration contracts |
| Closure gate checklist | Run full definition-of-done (section 8 below) |

**Artifacts:** Code cleanup across all modules. Updated `CLAUDE.md`.

---

## 4. Dependencies / Order of Operations

```
Phase 1 (blocking gaps)
  │
  ├──► Phase 2 (tests) ──► Phase 3 (curated memory completion)
  │                              │
  │                              ├──► Phase 4 (provider tightening)
  │                              │
  │                              └──► Phase 5 (council hardening)
  │                                        │
  └────────────────────────────────────────►│
                                            ▼
                                      Phase 6 (polish & close)
```

- **Phase 1 must complete first** — workspace targeting fix is load-bearing for everything else.
- **Phase 2 should complete before 3-5** — tests provide the safety net for subsequent changes.
- **Phases 3, 4, 5 are semi-parallel** — they touch different modules but share types. Sequence 3 → 4 → 5 is safest.
- **Phase 6 is strictly last** — closure gate validates everything.

---

## 5. Explicit Deliverables per Phase

| Phase | Files Changed | New Files | New Commands |
|-------|--------------|-----------|--------------|
| 1 | `config.rs`, `commands.rs` | — | — |
| 2 | All `src/*.rs` (test modules) | `tests/fixtures/*.jsonl` (optional) | — |
| 3 | `commands.rs`, `projects.rs`, `memory.rs` | — | `curated backfill`, `curated archive`, `curated list` |
| 4 | `memory.rs`, `graph.rs`, `commands.rs` | — | — |
| 5 | `commands.rs`, `memory.rs`, `types.rs` | — | `curated promote` |
| 6 | All (cleanup) | — | — |

---

## 6. Testing / Validation Strategy

### Unit Tests (Phase 2)
- Pure function tests: routing classification, memory ranking, synthesis bucketing, graph normalization
- No external dependencies — all inputs are in-memory structs or fixture strings
- Target: every public function in `routing.rs`, `memory.rs`, `synthesis.rs`, `graph.rs`, `projects.rs` has at least one test

### Integration Tests (Phase 2+)
- Fixture-based: seed JSONL files in a temp directory, run CLI commands, assert output
- No live Memoryport or GitNexus — test the fallback paths
- Optional: if `uc`/`gitnexus` are available, run a "full stack" test (not required for CI)

### Validation Command (Continuous)
- `layers validate` is the built-in health check
- Must pass with: structured memory only (no semantic, no graph) — degraded but healthy
- Must pass with: all providers available — full health
- Must fail clearly with: corrupt JSONL, missing workspace

### Parity Check (Phase 1 gate)
- Run `layers query`, `layers validate` on a prepared repo
- Compare output structure to Python prototype (if still available)
- Document any intentional divergences

---

## 7. What to Defer

These are explicitly **out of scope** for this closure track:

| Deferred Item | Why |
|---------------|-----|
| Provider trait abstraction | Two providers with stable CLI contracts don't need a trait. Concrete functions are simpler and sufficient. |
| Dynamic plugin system | No third-party providers exist. YAGNI. |
| Embedding generation | Memoryport's responsibility, not Layers'. |
| Council orchestration | Role enforcement, convergence management, multi-model coordination — this is Triumvirate's job. |
| Multi-repo support | One workspace per Layers instance is the design constraint. |
| GUI/TUI | CLI + JSON output covers all current consumers. |
| Cloud/network providers | Violates local-first principle. |
| Automatic memory ingestion | Violates explicit-over-implicit principle. |
| Record update/supersession | Append-only with archive is sufficient. Full update semantics add complexity without clear benefit. |
| Task dependencies | Project/task records are informational, not workflow engines. |

---

## 8. Definition of Done

The Layers core track is **done** when all of the following are true:

### Code Quality
- [ ] `cargo build --release` succeeds with no warnings
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] No `TODO`, `FIXME`, or `unimplemented!()` in source

### Functionality
- [ ] `layers query` returns ranked results from curated memory without external services
- [ ] `layers query` integrates Memoryport and GitNexus results when available
- [ ] `layers curated import` ingests records with deduplication
- [ ] `layers curated list` displays filtered canonical records
- [ ] `layers curated archive` soft-deletes records
- [ ] `layers curated backfill` promotes council artifacts to curated store
- [ ] `layers curated promote` extracts structured records from council artifacts
- [ ] `layers remember` persists plan/learning/trace with field validation
- [ ] `layers validate` passes in degraded mode (no external services)
- [ ] `layers validate` passes in full mode (all providers available)
- [ ] Workspace root is correctly derived from current repo, not hardcoded

### Test Coverage
- [ ] Unit tests for: routing (all branches), memory ranking (tier priority + dedup), curated import (happy + error), synthesis (bucket limits), graph normalization
- [ ] Integration tests with fixture data (no live services required)
- [ ] All tests pass in CI-equivalent environment

### Documentation
- [ ] CLI `--help` text is complete and includes examples for all commands
- [ ] `CLAUDE.md` reflects the final CLI surface
- [ ] Error messages are actionable (what failed, why, what to do)

### Closure Gate
- [ ] No open parity gaps with Python prototype
- [ ] Audit log format is stable and documented
- [ ] JSONL schemas are stable (no planned field additions)
- [ ] All deferred items (section 7) are documented, not dangling

When every box is checked, this track moves to **maintenance-only**: bug fixes and external contract updates only. No new features.

---

## 9. Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| **Memoryport CLI contract changes** | Medium | High — breaks semantic retrieval | Pin expected output format in tests. `validate` checks version. Fallback to structured-only retrieval is always available. |
| **GitNexus output schema drift** | Medium | Medium — breaks graph normalization | Pin JSON response schema in tests. Version check in `validate`. Graph is optional — queries still work without it. |
| **Scope creep from council workflow** | High | High — reopens the track | Council orchestration is Triumvirate's job. Layers only persists and retrieves artifacts. Hard boundary: no session management, no role enforcement, no convergence logic in Layers. |
| **Over-engineering provider abstraction** | Medium | Medium — delays closure | Concrete functions over traits. Two providers with stable contracts don't need abstraction. If a third provider appears, reconsider — but not before. |
| **Backfill produces low-quality curated records** | Medium | Low — curated store gets noisy | Backfill requires human confirmation. Promote command is explicit, not automatic. Archive command provides cleanup path. |
| **Test fixtures diverge from real data** | Low | Medium — false confidence | Fixtures derived from actual `curated-memory.jsonl` and real GitNexus output. Periodically refresh. |
| **Workspace targeting fix breaks existing setups** | Low | Medium — users can't query | Add fallback chain: explicit `--workspace` flag → env var → git ancestor walk → error. Test all paths. |

---

## Timeline Summary

| Phase | Days | Effort | Cumulative |
|-------|------|--------|------------|
| 1. Blocking gaps | 1-2 | Light | 2 days |
| 2. Test suite | 3-5 | Medium | 5 days |
| 3. Curated memory | 6-8 | Medium | 8 days |
| 4. Provider tightening | 9-11 | Medium | 11 days |
| 5. Council hardening | 12-13 | Light | 13 days |
| 6. Polish & close | 14-15 | Light | 15 days |

**Total: ~15 working days to closure.** After that, maintenance-only.
