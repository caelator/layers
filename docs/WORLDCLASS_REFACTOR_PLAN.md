# Layers world-class refactor plan

## North star

Refactor Layers into a local-first Rust context compiler/context spine for coding agents. `ContextPacket` is the product artifact. Query, preflight, impact, MCP, and autoresearch should all feed or render the same packet compiler pipeline instead of owning parallel packet semantics.

## Guardrails

- Preserve CLI flags, JSON field names, serialized schemas, and default command output unless a task explicitly updates tests and docs.
- Keep `cargo fmt --all --check`, `cargo test --workspace --all-targets`, and `cargo clippy --workspace --all-targets -- -D warnings` green after each batch.
- Prefer private/internal extraction before public API moves.
- Run GitNexus impact before editing symbol-heavy code.
- Avoid committing local runtime artifacts such as `memoryport/autoresearch.sqlite`, `memoryport/telemetry/events.jsonl`, `.hermes/`, or ad-hoc dogfood runs unless intentionally reviewed.

## Architecture target

### Stable core

- `layers-core`: schema types and pure helpers for `ContextPacket`, memory records, impact records, warning/diagnostic records, and common errors.
- Context compiler module/crate: packet assembly, source adapters, evidence rendering, workspace snapshots, finalization invariants.
- Store layer: canonical local repositories for curated memory, sessions, and packet/cache data.
- Impact adapter: GitNexus/git fallback normalization into structured packet sections.
- MCP stable tools: `layers.query`, `layers.preflight`/`layers.context_packet`, `layers.impact`, memory list/search/show/remember, validate/doctor.

### Experimental/deprecated compatibility

- Agent runtime, providers, daemon, channels, and generic tools remain available during transition, but should stop defining the product shape.
- CLI wrappers may stay for compatibility; the implementation should increasingly call stable compiler APIs.

## Refactor batches

### Batch 1: narrow internal cleanup in `crates/layers-store/src/sqlite.rs`

Goal: prove the refactor loop is safe in the current dirty tree.

Tasks:
1. Deduplicate DB-worker channel error construction into private helpers.
2. Preserve exact `BrokenPipe` kind and messages: `db worker gone`, `db worker dropped reply`.
3. Avoid schema, trait, SQL, or public API changes.

Verification:
- `cargo test -p layers-store sqlite`
- `cargo clippy -p layers-store --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`

### Batch 2: packet finalization invariants

Goal: make packet consistency automatic.

Tasks:
1. Add pure helpers around `ContextPacket` section/trace finalization.
2. Centralize selection trace refresh from sections/items.
3. Centralize open-uncertainty derivation from warnings.
4. Keep legacy fields intact.

Verification:
- Unit tests for idempotent finalization and trace coverage.
- Focused query/preflight/autoresearch tests.
- Full workspace gates.

### Batch 3: shared `ContextItem` construction

Goal: remove duplicated item construction while preserving packet shape.

Tasks:
1. Introduce an internal builder/helper for cited context items.
2. Replace duplicate constructors in query, preflight, and autoresearch.
3. Preserve item IDs, source kinds, selected reasons, token estimates, and tags.

Verification:
- Existing packet JSON tests.
- New tests for source/provenance preservation.
- Full workspace gates.

### Batch 4: extract packet compiler module inside the binary crate

Goal: make commands thin wrappers without a crate split yet.

Tasks:
1. Add `src/context_packet_compiler/` with builder, evidence renderer, workspace metadata, and source adapters.
2. Move pure packet assembly out of `src/cmd/query.rs` and `src/cmd/preflight.rs` behind compatibility functions.
3. Keep side effects in command modules: audit, telemetry, printing, CLI parsing.

Verification:
- Snapshot/shape tests for query and preflight JSON.
- Focused autoresearch/preflight/query tests.
- Full workspace gates.

### Batch 5: derive legacy evidence from packet sections

Goal: stop maintaining evidence as a separate source of truth.

Tasks:
1. Add an evidence renderer that walks packet sections/items.
2. Preserve existing headings and important text where possible.
3. Include source URI, source kind, freshness/reliability/provenance for autoresearch items.
4. Render once during packet finalization.

Verification:
- Fixture tests for memory, GitNexus, autoresearch, and truncated evidence.
- Full workspace gates.

### Batch 6: autoresearch provider/adapter split

Goal: keep autoresearch as a context input source, not a standalone research product.

Tasks:
1. Split store/search/prepare logic from CLI printing.
2. Expose `prepare_findings(task, targets, limit)` and `findings_to_context_section`.
3. Keep existing `add_autoresearch_to_packet` as a compatibility wrapper until commands migrate.
4. Add unavailable-store and empty-findings tests.

Verification:
- Focused autoresearch tests.
- Query/preflight bridge tests.
- Full workspace gates.

### Batch 7: structured workspace and impact sections

Goal: make code-heavy context packets include blast radius and local state consistently.

Tasks:
1. Reuse preflight workspace state collection from query for code-heavy tasks.
2. Normalize GitNexus impact output into structured packet items.
3. Add degraded-mode warnings for unavailable/stale/empty GitNexus output.
4. Add stable `layers impact <target>` behavior if not already exposed.

Verification:
- Fixture tests with canned GitNexus JSON.
- Degraded-mode tests with GitNexus unavailable.
- Full workspace gates.

### Batch 8: canonical memory API

Goal: make curated memory typed, inspectable, and retireable.

Tasks:
1. Define stable memory record kinds/status/confidence/source fields.
2. Keep council JSONL files as legacy read adapters.
3. Make canonical curated memory the primary read API.
4. Add list/search/show/audit command tests before changing writes.

Verification:
- Memory search/list fixtures.
- Backward-compat legacy file tests.
- Full workspace gates.

### Batch 9: stable MCP surface

Goal: expose the context compiler, not a generic agent runtime.

Tasks:
1. Add a stable MCP registry for context/memory/impact/validate tools.
2. Keep generic fs/process/subagent tools behind explicit opt-in.
3. Test that deprecated runtime tools are not exposed by default.

Verification:
- MCP server tool-list tests.
- Context tool call fixture tests.
- Full workspace gates.

### Batch 10: feature-gate deprecated runtime gravity

Goal: let the stable context compiler build without old runtime/product baggage.

Tasks:
1. Introduce compatibility features without changing defaults.
2. Move daemon/runtime/provider/channel imports behind features.
3. Add a stable-core-only check target once dependency boundaries permit it.
4. Document stable vs experimental crates.

Verification:
- Default full workspace gates.
- Stable-core-only check gate.

## First implementation target

Start with Batch 1 now. It is intentionally narrow, private, and easy to verify while the worktree is already broad and dirty.
