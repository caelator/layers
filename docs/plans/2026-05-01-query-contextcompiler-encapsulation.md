# Query ContextCompiler Encapsulation Implementation Plan

> **For Hermes:** Use test-driven-development and small verified slices.

**Goal:** Route `layers query` packet assembly through `layers-compiler::ContextCompiler` without losing query-specific route labels, provenance surface, telemetry, or compatibility fields.

**Architecture:** Keep side-effectful query retrieval in `src/cmd/query.rs`, but make final packet normalization an explicit compiler handoff. The query command assembles sections/warnings/metadata, then transfers the pure packet fields into `CompileRequest`; `ContextCompiler` owns route/surface stamping, provenance consistency, selection trace, open uncertainty, and evidence derivation.

**Tech Stack:** Rust workspace, `layers-core::ContextPacket`, `layers-compiler::{CompileRequest, CompileMode, ContextCompiler}`, Cargo tests.

---

### Task 1: Lock query route/surface behavior

**Objective:** Add focused coverage proving query packets preserve router labels while provenance identifies the public surface as `query`.

**Files:**
- Modify: `src/cmd/query.rs`

**Steps:**
1. Add a test in `src/cmd/query.rs` that calls `build_context_packet` with `Route::MemoryOnly`.
2. Assert `packet.route == "memory_only"`.
3. Assert `packet.provenance.surface == "query"`.
4. Assert finalization still populates compatibility fields such as `selection_trace` and `open_uncertainty` from warnings where relevant.
5. Run: `cargo test -q query_context_packet_preserves_route_label_and_query_surface -- --nocapture` and confirm it fails before the implementation is fixed.

### Task 2: Fix the compiler handoff in query

**Objective:** Make `build_context_packet` pass correct stable `ContextPacket` fields into `CompileRequest`.

**Files:**
- Modify: `src/cmd/query.rs`

**Steps:**
1. Replace `packet.packet_id` with `packet.id`.
2. Replace `packet.generated_at` with `packet.created_at`.
3. Keep `.with_route_label(route.label())` so query route labels are preserved.
4. Move sections/warnings into the request with `std::mem::take`.
5. Copy query-only compatibility fields back onto the compiled packet after compilation.
6. Run the focused query test and `cargo test -q build_context_packet -- --nocapture`.

### Task 3: Clean compiler helper exports

**Objective:** Remove obsolete query-local dependency on `finalize_packet` while keeping compiler crate as the owner of packet normalization.

**Files:**
- Modify: `src/context_packet_compiler/mod.rs`

**Steps:**
1. Remove `finalize_packet` from the command shim re-export if no command module imports it through the shim.
2. Keep direct compiler tests for `finalize_packet` in `crates/layers-compiler`.
3. Run `cargo check --workspace --all-targets` to catch unused exports/imports.

### Task 4: Verify the slice

**Objective:** Prove the change is correct, formatted, warning-free, and reviewable.

**Commands:**
- `cargo fmt --all --check`
- `cargo test -q query_context_packet_preserves_route_label_and_query_surface -- --nocapture`
- `cargo test -q build_context_packet -- --nocapture`
- `cargo test -q -p layers-compiler compiler_request_can_preserve_query_route_label -- --nocapture`
- `cargo check --no-default-features --all-targets`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `git diff --check`
- `cargo test --workspace --all-targets`

### Task 5: Review and commit

**Objective:** Commit only after verification and security/diff scan.

**Steps:**
1. Review `git diff --stat` and relevant diffs.
2. Scan added lines for secret/shell-injection patterns.
3. Commit with `[verified] Route query packets through ContextCompiler` if all gates pass.
