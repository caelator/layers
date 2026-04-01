# Phase III Evaluation: Council Workflow Inside Layers

**Evaluator:** Claude Opus 4.6
**Date:** 2026-03-31
**Repo state:** All tests green, `cargo test` passing, no uncommitted src changes

---

## Executive Summary

Phase III is **substantially implemented** but **not honestly complete**. The three-stage council execution path works end-to-end, artifacts are persisted, retry/timeout machinery exists, and grounding via curated memory + GitNexus is wired in. What's missing is hardening: the convergence contract is fragile, stall detection is absent, several defined data structures are never populated, test coverage skips failure modes, and operator-facing status conflates "ran" with "succeeded."

**Honest assessment: ~70% complete.** The remaining 30% is not speculative — it's the difference between "it works on the happy path" and "it's reliable enough to trust."

---

## What Is Already Solid

### 1. Core Council Execution Engine (`council.rs`, 606 lines)
- Fixed Gemini -> Claude -> Codex stage order is implemented and tested
- Each stage receives prior stage outputs (proper chaining)
- Shell command spawning with environment variables (LAYERS_COUNCIL_STAGE, MODEL, ROLE, etc.)
- Process lifecycle management with 100ms polling

### 2. Durable Artifact Structure
- Run artifacts at `~/.memoryport/council-runs/{run_id}/`
- Per-run: `run.json`, `context.txt`, `context.json`, `convergence.json`
- Per-stage: `{stage}-prompt.txt`, `{stage}-attempt-{N}.stdout.txt`, `{stage}-attempt-{N}.stderr.txt`
- `run.json` persisted after every state change (crash-recoverable in principle)
- JSONL trace records appended to `council-traces.jsonl`

### 3. Retry and Timeout Mechanics
- Configurable `retry_limit` and `timeout_secs` per run
- Retry loop with explicit status transitions: running -> succeeded | failed | timed_out
- Process killed on timeout with explicit error message
- Attempt records preserved across retries (full audit trail)

### 4. Memory Grounding (`synthesis.rs`, `memory.rs`)
- `synthesize_memory_brief()` extracts typed hits: decisions (max 2), constraints (max 2), status (max 2), next steps (max 2), postmortems (max 1), notable context (max 3)
- Source weighting: curated-memory (6) > structured-records (3) > memoryport-semantic (2) > jsonl-fallback (1)
- Low-signal filtering removes noise (< 12 chars, placeholder text)
- Fallback messaging when hits exist but no canonical status matches

### 5. GitNexus Grounding (`graph.rs`)
- Index version, blast radius, risk level, affected processes surfaced into graph context
- Impact summary computed per target via `gitnexus impact {target} --direction upstream`
- Risk aggregation tracks highest risk across targets

### 6. Typed Records (`types.rs`, 363 lines)
- `CouncilRunRecord`, `CouncilStageRecord`, `CouncilStageAttempt`, `CouncilConvergenceRecord` — well-structured
- `MemoryBrief` with decisions, constraints, status, next_steps, postmortems, notable
- `GraphContext` with index version, impact summary

### 7. Test Coverage (Happy Path)
- `council_run_executes_fixed_stage_order_and_persists_artifacts` — validates 3-stage execution, artifact creation, convergence parsing
- `council_run_retries_failed_stage_once` — validates retry on transient failure
- `typed_hits_drive_memory_brief` — validates memory brief extraction
- `search_memory_prefers_curated_hits` — validates source precedence
- Routing tests and end-to-end validation test

---

## What Is Missing or Incomplete

### Critical (Must Fix for Honest Completion)

#### C1. Fragile Convergence Contract
**Location:** `council.rs` lines 381-410
**Problem:** Convergence detection relies on a case-insensitive string match for `"Convergence: converged"` in Codex output. No validation that the output contains the expected `## Decision`, `## Risks`, `## Next Steps` sections. Summary extraction falls back to "first non-empty line" — unreliable.
**Risk:** A Codex stage that outputs garbage with the magic string is marked "converged." A well-reasoned output without the exact marker is marked "incomplete."
**Fix:** Validate that convergence output contains at minimum a decision summary and next steps. Parse structured sections rather than a magic marker.

#### C2. No Stall Detection
**Location:** `council.rs` lines 269-379
**Problem:** A stage that exits 0 with trivially short output (e.g., "ok") is treated as success. There is no minimum output quality check beyond "non-empty."
**Risk:** A stage that produces a greeting or error message (not empty, exit 0) silently poisons downstream stages.
**Fix:** Add minimum word-count threshold for stage output. Flag suspiciously short output as a stall/quality failure.

#### C3. `ImplementationContext` and `ReviewContext` Never Populated
**Location:** `types.rs` lines 59-77, `commands.rs` line 137
**Problem:** Both structs are defined with fields (target_symbols, changed_files, affected_flows / before_scope, after_scope, drift_symbols) but always set to `None`. These were designed to carry GitNexus context into council runs.
**Risk:** The graph context is thinner than the type system promises. Code that consumes these fields silently gets nothing.
**Fix:** Either populate them from GitNexus data or remove the dead structures to avoid false promises.

#### C4. Operator Status Conflation
**Location:** `council.rs`, `commands.rs`
**Problem:** Run status is either `"completed"`, `"incomplete"`, or `"failed"` — but there's no distinction between "converged and verified" vs "stages ran to completion but convergence marker missing" vs "partially completed with some stages failed." The terminal output doesn't clearly explain *why* a run ended in its state.
**Risk:** Operator cannot quickly distinguish between "council disagreed" and "infrastructure failure."
**Fix:** Add explicit terminal reason codes: `converged`, `not_converged`, `stage_failed`, `stage_timed_out`, `retries_exhausted`.

### Important (Should Fix)

#### I1. No Tests for Failure Modes
**Problem:** No tests for: timeout behavior, empty output handling, retry exhaustion, synthesis context building, graph impact computation.
**Fix:** Add at minimum: timeout test, empty-output-as-failure test, all-retries-exhausted test.

#### I2. Context Size Hard Bail
**Location:** `synthesis.rs` lines 189-190
**Problem:** If context exceeds 1200 words, the entire query fails. No intelligent truncation or prioritization.
**Fix:** Implement truncation that preserves high-weight items (decisions, constraints) and drops lower-weight items (notable context) until within budget.

#### I3. No Curated Memory Promotion Path
**Problem:** Council outcomes (decisions, learnings) are written to `council-plans.jsonl` and `council-learnings.jsonl` but there's no mechanism to promote a council decision into canonical curated memory. The council produces artifacts but doesn't close the loop.
**Fix:** Add a promotion command or flag that creates a curated-memory record from a council convergence outcome.

#### I4. Stage Failure Cascade (No Partial Results)
**Location:** `council.rs` line ~340
**Problem:** If Gemini fails, the entire council run fails. No option for Claude/Codex to proceed with reduced context, and no partial result recording.
**Fix:** Consider recording partial results and allowing operator to resume from last successful stage. At minimum, ensure partial artifacts are preserved and queryable.

### Deferrable (Won't Lie About Completion)

- **D1. Prompt injection surface** — Task text interpolated directly into prompts. Low risk in local-first context but worth noting.
- **D2. Graph provider fallback** — No fallback to git-based analysis if GitNexus unavailable. Acceptable for Phase III scope.
- **D3. Automatic target detection** — GitNexus impact only computed when targets are explicitly provided. Reasonable for now.
- **D4. Dynamic plugin loading** — Not in scope per v2 architecture decision.

---

## Test Adequacy Assessment

| Area | Coverage | Verdict |
|------|----------|---------|
| Happy-path council execution | Tested | OK |
| Retry on transient failure | Tested | OK |
| Timeout handling | **Not tested** | Gap |
| Empty output handling | **Not tested** | Gap |
| Retry exhaustion | **Not tested** | Gap |
| Memory brief extraction | Tested | OK |
| Curated source precedence | Tested | OK |
| Synthesis context building | **Not tested** | Gap |
| Graph impact computation | **Not tested** | Gap |
| Routing classification | Tested | OK |
| End-to-end validation | Tested | OK |
| Convergence parsing (edge cases) | **Not tested** | Gap |

---

## Grounding Assessment

| Grounding Source | Wired In | Actually Used | Quality |
|-----------------|----------|---------------|---------|
| Curated memory (decisions, constraints) | Yes | Yes | Good — weighted, filtered, capped |
| Structured records (plans, learnings) | Yes | Yes | Adequate — lower weight but present |
| JSONL fallback (traces) | Yes | Yes | Adequate as fallback |
| GitNexus index version | Yes | Yes | Good |
| GitNexus impact/blast radius | Yes | Partially | Missing: implementation_context, review_context always None |
| GitNexus affected processes | Yes | Yes | Good |

---

## Bottom Line

The council workflow **works**. The architecture is sound, the stage execution is clean, artifacts are durable, and grounding is real. What prevents calling Phase III complete is:

1. **Convergence is a magic string, not a verified contract** (C1)
2. **No stall detection** means garbage-in passes silently (C2)
3. **Dead type structures** create false confidence in graph context depth (C3)
4. **Operator can't distinguish failure modes** from terminal status alone (C4)
5. **Test coverage only validates the happy path** (I1)

None of these require architectural changes. They're hardening tasks — the kind of work that separates "demo" from "tool."
