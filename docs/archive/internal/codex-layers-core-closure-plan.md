# Layers Core Closure Plan

## Objective

Close the remaining Layers core / Rust track without reopening architecture scope. The finish line is a small, reliable Rust core for three things only:

- council workflows
- Memoryport continuity through curated canonical records
- GitNexus-backed code understanding

Everything below is organized around shipping that cleanly, not expanding the product surface.

## Current Baseline

The repo already appears to have the important first slices in place:

- curated memory has a canonical JSONL store at `memoryport/curated-memory.jsonl`
- project/task-style structured records exist in `memoryport/project-records.jsonl`
- retrieval already ranks structured/canonical memory ahead of fallback memory
- the Rust CLI is split across focused modules: `memory`, `projects`, `graph`, `routing`, `synthesis`, `commands`
- `validate` and core tests have already been exercised in prior closure work

That means the remaining work is mostly closure work:

- remove dual-path ambiguity between canonical and legacy records
- finish ingestion and backfill as an operational path, not a partial import
- tighten the boundaries between Layers and Memoryport/GitNexus
- make council workflows use the same durable record model
- harden the Rust implementation with deterministic tests and finish criteria

## Closure Principles

- Prefer one canonical write path per durable record type.
- Keep compatibility reads only long enough to migrate/backfill safely.
- Ship explicit workflows, not a generic PM or platform layer.
- Treat provider subprocesses as acceptable, but wrap them behind stable typed boundaries.
- Define closure by operational behavior and tests, not by further architecture documents.

## Phase 1: Canonical Curated Memory Closure

### Goal

Make curated memory the single first-class record system for durable Memoryport retrieval.

### Priority

Highest. Other phases depend on this boundary being stable.

### Required work

- Finalize the canonical curated record schema and document allowed record kinds.
- Make one write path authoritative for curated records:
  - import
  - remember/promote flows
  - any workflow-derived durable writeback
- Keep legacy `project-records` reads only as a migration shim, not as an equal source of truth.
- Add explicit migration/backfill logic so old records can be re-expressed as canonical curated records once, then verified.
- Ensure retrieval, synthesis, and audit output identify canonical curated hits clearly.

### Dependencies

- current curated import path in Rust
- existing typed record structs in `types.rs` / `projects.rs`
- current search ordering in `memory.rs`

### Risks

- dual-read behavior can hide schema drift and let stale legacy data shadow canonical records
- duplicate or conflicting records can make retrieval appear nondeterministic
- premature deletion of compatibility reads can strand existing local data

### Finish criteria

- `memoryport/curated-memory.jsonl` is the only canonical store for curated memory records.
- Every supported durable curated write path lands in the canonical file.
- Legacy project/task record compatibility is either:
  - fully migrated and disabled for retrieval, or
  - explicitly marked as temporary read-only migration support with a removal condition.
- Equivalent queries return curated hits from the canonical store without depending on legacy files.
- Tests cover load, dedupe, import, and retrieval ranking for canonical curated records.

## Phase 2: Ingestion And Backfill Completion

### Goal

Turn the current ingest/import work into a repeatable operational path that can populate and maintain canonical curated memory.

### Priority

Highest, immediately after the canonical boundary is fixed.

### Required work

- Define the supported source inputs for ingestion/backfill:
  - distilled JSONL imports
  - legacy project/task records if still present
  - workflow outputs that should promote durable lessons
- Make backfill idempotent:
  - stable IDs or stable dedupe keys
  - clear duplicate handling
  - resumable imports
- Add validation/reporting for ingest runs:
  - imported
  - skipped
  - conflicted
  - failed
- Add a CLI/operator path for full backfill plus targeted import.
- Verify retrieval quality after backfill, not just record counts.

### Dependencies

- Phase 1 canonical schema and canonical file ownership
- stable typed conversion logic from source artifacts to curated records

### Risks

- ingest can become a one-off script path instead of a maintained product path
- backfill may preserve structurally valid but low-value records that hurt retrieval quality
- non-idempotent imports create local trust problems quickly

### Finish criteria

- A fresh workspace can be backfilled into canonical curated memory through one documented CLI path.
- Re-running the same ingest/backfill does not create duplicate logical records.
- Ingest results are auditable and summarized in machine-readable output.
- Retrieval after backfill surfaces the expected canonical memory hits for representative council and code-understanding queries.

## Phase 3: Memoryport Integration Tightening

### Goal

Make Memoryport a clean first-class provider around curated records instead of a loose set of ad hoc JSONL behaviors.

### Priority

High. This determines whether closure is durable or only locally working.

### Required work

- Separate canonical local record handling from external Memoryport provider behavior in code.
- Clarify provider responsibilities:
  - Layers owns normalized record types, ranking, and workflow orchestration.
  - Memoryport provider owns import/export/query interaction details.
- Make provider health and readiness deterministic in `validate`.
- Standardize error handling and degraded-mode reporting when Memoryport is unavailable.
- Ensure audit events report which memory tier answered the query:
  - canonical curated
  - semantic provider
  - fallback local memory

### Dependencies

- Phases 1 and 2
- current `memory.rs`, `commands.rs`, and `config.rs` boundaries

### Risks

- provider probing can leak environment-specific behavior back into core validation
- canonical local storage and external provider logic can remain entangled, making future fixes risky

### Finish criteria

- The Rust code has a clear provider boundary for Memoryport operations.
- `validate` can distinguish contract health from environment-specific readiness without ambiguity.
- Query/audit output consistently reports memory source selection.
- Loss of Memoryport availability degrades predictably without corrupting local curated retrieval.

## Phase 4: GitNexus Operational Workflow Closure

### Goal

Keep GitNexus first-class, but make it operationally reliable for real code workflows rather than a thin integration.

### Priority

High, but after the memory path is closed.

### Required work

- Tighten the typed graph provider boundary so subprocess output normalization is isolated.
- Add tests for the GitNexus-dependent normalization paths already identified as important:
  - graph status normalization
  - `process_symbols` normalization
  - unborn-repo handling
  - context assembly from graph facts
- Ensure `validate` and query-time graph collection fail clearly and compactly.
- Keep GitNexus focused on code understanding flows:
  - architecture/context lookup
  - impact analysis support
  - process tracing

### Dependencies

- current `graph.rs` and validation behavior
- existing normalization and audit contract

### Risks

- GitNexus operational support can drift into a general platform management surface
- weak normalization tests can reintroduce machine-specific behavior

### Finish criteria

- Representative GitNexus query and validate flows are covered by automated tests with mocked subprocess outputs.
- The Rust implementation produces stable graph facts and stable audit fields across prepared machines.
- GitNexus usage in Layers remains limited to code understanding workflows, not general task/platform orchestration.

## Phase 5: Council Workflow Closure

### Goal

Support the workflows Layers is actually for: plan, handoff, postmortem, and durable council continuity.

### Priority

Medium-high. Needed for product closure, but should stay narrow.

### Required work

- Make the workflow artifact set explicit and small:
  - `workflow.plan`
  - `workflow.handoff`
  - `workflow.postmortem`
- Ensure workflows can promote durable conclusions into canonical curated memory when appropriate:
  - decisions
  - constraints
  - status
  - next steps
  - postmortem lessons
- Make synthesis output reference workflow artifacts and curated memory distinctly.
- Add at least one end-to-end flow per supported workflow:
  - gather context
  - synthesize artifact
  - optionally promote durable records

### Dependencies

- canonical curated record closure
- stable provider boundaries for memory and graph data
- synthesis and command wiring

### Risks

- workflow support can sprawl into a project-management subsystem
- promotion rules can become implicit and hard to trust if not constrained

### Finish criteria

- Each supported workflow has an explicit command/path and artifact shape.
- Durable lessons from council workflows can be promoted into canonical curated memory through an explicit path.
- There is no separate PM/work-tracking subsystem added beyond what council continuity actually needs.

## Phase 6: Rust Hardening And Replacement Gate

### Goal

Decide closure based on deterministic behavior and tests, not on “seems close.”

### Priority

Always in parallel, but finalized last.

### Required work

- Expand automated coverage around the remaining contract-sensitive logic:
  - routing behavior
  - memory ranking / low-signal filtering
  - context assembly
  - provider normalization
  - ingest/backfill idempotency
- Add small integration tests with mocked `uc` and `gitnexus` subprocess outputs.
- Keep `validate` as an operator check, not as the only proof of correctness.
- Re-run parity checks against the Python/prototype behavior only where it still matters to closure:
  - routing
  - evidence structure
  - audit fields
  - retrieval behavior

### Dependencies

- all prior phases for stable behavior to lock in

### Risks

- validation-only confidence will hide regressions in ranking and normalization
- continued prototype comparison beyond contract-level parity will waste closure time

### Finish criteria

- Core contract behavior is covered by unit/integration tests.
- `cargo test` and `cargo run -- validate` are both meaningful and reproducible on a prepared machine.
- Rust and the replaced prototype are materially equivalent on the still-supported contract-level outputs.
- No remaining open item is “must-have for councils/Memoryport/GitNexus” rather than “nice to improve later.”

## Recommended Execution Order

1. Canonical curated memory closure
2. Ingestion/backfill completion
3. Memoryport provider tightening
4. GitNexus operational hardening
5. Council workflow closure
6. Final Rust hardening and replacement gate

This order minimizes rework. If canonical curated storage is still ambiguous, everything above it stays provisional.

## What Counts As Done

The Layers core track should be considered closed when all of the following are true:

- curated memory has one canonical durable store and one authoritative write path
- ingestion/backfill is repeatable, idempotent, and auditable
- Memoryport and GitNexus are both first-class through typed provider boundaries
- council workflows produce explicit artifacts and can promote durable curated records
- the Rust code has deterministic tests around routing, retrieval, normalization, and ingest
- replacement readiness is based on stable contract behavior, not local luck
- no open required item would expand Layers beyond councils, Memoryport continuity, or GitNexus code understanding

## Explicit Deferrals

These should remain out of scope for closure unless they become necessary to satisfy the finish criteria above:

- generic PM/project management expansion
- task dependency systems
- workflow engines or autonomous orchestration
- plugin/platform abstractions
- replacing Memoryport or GitNexus with native internal systems
- richer ranking/ontology work beyond what is needed for stable retrieval quality
- any new artifact type beyond plan, handoff, postmortem, and canonical curated memory records
