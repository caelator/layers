# Phase III Completion Plan

**Author:** Claude Opus 4.6
**Date:** 2026-03-31
**Baseline:** Evaluation at `claude-phase3-evaluation.md`
**Scope:** Hardening only. No new features, no architectural changes.

---

## Completion Criteria

Phase III is honestly complete when:

1. A grounded council run produces verified convergence with all artifacts present and internally consistent
2. A non-converged run is clearly labeled with the reason (not just "incomplete")
3. Timeout, stall, empty output, and retry exhaustion are handled and tested
4. Dead type structures are either populated or removed
5. An operator can look at terminal output and understand what happened without reading artifact files

---

## Work Items (Ordered by Priority)

### Step 1: Strengthen Convergence Contract

**Files:** `council.rs`, `types.rs`
**Effort:** Medium

Replace the magic-string convergence detection with structured output validation:

1. After Codex stage succeeds, parse output for required sections: at minimum `## Decision` and `## Next Steps`
2. Extract decision summary from `## Decision` section (not just first line)
3. Extract unresolved items from `## Risks` section (already done, keep it)
4. Extract next steps from `## Next Steps` section into `CouncilConvergenceRecord`
5. Mark as `converged` only if decision section is non-empty and contains substantive content (> 20 words)
6. Mark as `partial` if some sections present but decision missing
7. Keep `incomplete` for empty/trivial output
8. Add `next_steps: Vec<String>` to `CouncilConvergenceRecord` if not already present

**Acceptance:** Test that well-formed output -> converged, missing-decision output -> partial, empty output -> incomplete.

### Step 2: Add Stall Detection

**Files:** `council.rs`
**Effort:** Small

After a stage exits 0 with non-empty output, check output quality:

1. Count words in stdout. If < 15 words, treat as stall: set status to `"stalled"`, error to `"stage output too short ({N} words, minimum 15)"`
2. Apply same retry logic as other failures
3. Record stall in attempt record for audit

**Acceptance:** Test that a stage producing "ok done" triggers stall detection and retry.

### Step 3: Add Explicit Terminal Reason Codes

**Files:** `council.rs`, `types.rs`
**Effort:** Small

Add `terminal_reason: Option<String>` to `CouncilRunRecord`. Set it on terminal state transitions:

| Condition | `status` | `terminal_reason` |
|-----------|----------|--------------------|
| Convergence achieved | `completed` | `converged` |
| All stages succeeded but no convergence | `incomplete` | `not_converged` |
| Stage failed after retries | `failed` | `retries_exhausted: {stage}` |
| Stage timed out after retries | `failed` | `timed_out: {stage}` |
| Stage stalled after retries | `failed` | `stalled: {stage}` |

Print terminal reason in CLI output alongside status.

**Acceptance:** Operator can distinguish all failure modes from terminal output alone.

### Step 4: Clean Up Dead Type Structures

**Files:** `types.rs`, `commands.rs`
**Effort:** Small

Decide for each:

- **`ImplementationContext`**: If gitnexus can provide `changed_files` and `affected_flows` from detect-changes output, wire it in. If not feasible in Phase III scope, **remove the struct** and the `Option<ImplementationContext>` field from `GraphContext`. Don't leave dead code.
- **`ReviewContext`**: Same treatment. If `before_scope` / `after_scope` require detect-changes with a base ref (which requires git state), this is Phase IV+ scope. Remove the struct.

**Acceptance:** No struct in types.rs that is always `None` in practice.

### Step 5: Add Failure-Mode Tests

**Files:** `council.rs` (test module)
**Effort:** Medium

Add these tests:

1. **Timeout test**: Stage command sleeps longer than timeout_secs. Assert: attempt status is `timed_out`, run status is `failed`, terminal_reason mentions timeout.
2. **Empty output test**: Stage command exits 0 but produces no stdout. Assert: treated as failure, retried if retry_limit > 0.
3. **Stall test**: Stage command exits 0 with < 15 words. Assert: treated as stall, retried.
4. **Retry exhaustion test**: Stage command fails on all attempts. Assert: run status is `failed`, terminal_reason is `retries_exhausted`, all attempt records preserved.
5. **Convergence edge cases**: (a) Output with decision section but no magic marker -> converged via section parsing. (b) Output with magic marker but no decision section -> partial. (c) Completely empty -> incomplete.

**Acceptance:** `cargo test` passes with all new tests green.

### Step 6: Context Size Graceful Degradation

**Files:** `synthesis.rs`
**Effort:** Small

Replace the hard bail on > 1200 words with truncation:

1. If context exceeds budget, drop items by ascending weight: notable context first, then postmortems, then status, then next_steps. Never drop decisions or constraints.
2. Append a note: `"[context truncated: {N} items dropped to fit budget]"`
3. Log a warning but don't fail the query.

**Acceptance:** A query with verbose memory hits still produces a context (truncated) rather than failing.

### Step 7: Curated Memory Promotion (Minimal)

**Files:** `commands.rs`, `council.rs`
**Effort:** Small-Medium

Add a `layers council promote --run-id {id}` subcommand:

1. Read `convergence.json` from the run's artifact directory
2. If status is `converged`, create a curated-memory record (kind: `decision`) from the convergence summary and next steps
3. Write to `curated-memory.jsonl`
4. Print confirmation with the promoted record

This closes the loop: council decides -> decision enters canonical memory -> future councils can see it.

**Acceptance:** A converged council run can be promoted to curated memory and appears in subsequent `layers query` results.

---

## What Can Be Deferred Without Lying

These items are real gaps but **not required for Phase III honest completion**:

| Item | Why Deferrable |
|------|---------------|
| Prompt injection hardening | Local-first context, no untrusted input |
| Graph provider fallback to git | GitNexus unavailability is an environment issue, not a workflow correctness issue |
| Automatic target detection from task text | Nice-to-have, explicit targets are the honest interface |
| Partial council resumption | Restart is acceptable for Phase III; resume is Phase IV+ |
| Dynamic stage ordering | Fixed order is the design decision, not a limitation |

---

## Execution Order and Dependencies

```
Step 1 (convergence contract)
  |
  v
Step 2 (stall detection)  -->  Step 3 (terminal reasons)
                                  |
                                  v
                           Step 4 (dead types cleanup)
                                  |
                                  v
                           Step 5 (failure-mode tests) -- covers Steps 1-4
                                  |
                                  v
                           Step 6 (context truncation)
                                  |
                                  v
                           Step 7 (memory promotion)
```

Steps 1-3 are tightly coupled (convergence contract changes affect what terminal reasons exist and what stall detection feeds into). Step 5 validates all of 1-4. Steps 6-7 are independent but lower priority.

---

## Definition of Done

Phase III is complete when:

- [ ] Convergence is validated by section parsing, not a magic string
- [ ] Stall detection catches trivially short output
- [ ] Terminal reason codes distinguish all failure modes
- [ ] No dead type structures remain in types.rs
- [ ] Failure-mode tests exist and pass for: timeout, empty output, stall, retry exhaustion, convergence edge cases
- [ ] Context truncation degrades gracefully instead of failing
- [ ] Council decisions can be promoted to curated memory
- [ ] `cargo test` is green
- [ ] `layers validate` passes

Estimated total effort: **1-2 focused sessions** (the changes are surgical, not architectural).
