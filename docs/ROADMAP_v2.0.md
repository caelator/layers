# Layers v2.0 Executable Roadmap — Minimal Context Compiler Release

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Ship the smallest credible Layers 2.0: a production-grade, local-first `ContextPacket` compiler that coding agents can call through CLI and MCP before editing.

**Architecture:** Layers 2.0 centers on one artifact, `ContextPacket`, and one engine, `layers-compiler`. CLI commands and MCP tools are thin adapters over the compiler. Deprecated runtime/daemon/provider/channel surfaces remain compatibility-only and must not be required for stable-core builds.

**Tech Stack:** Rust workspace, `layers-core`, new `layers-compiler`, `layers-mcp`, CLI command modules, local JSONL stores, Git/GitNexus where available, Cargo fmt/test/check/clippy gates.

---

## 1. Scope Decision

This roadmap is intentionally narrower than the strategic v2.x vision.

Layers 2.0 is not the full context operating system. It is the minimum release that proves the core thesis end-to-end:

1. `ContextPacket` v2 is stable and documented.
2. `query` and `preflight` compile packets through one shared compiler.
3. Packets can be validated, inspected, rendered, and consumed by agents.
4. MCP exposes stable compiler-backed context tools only.
5. Stable-core builds do not depend on deprecated runtime/daemon gravity.
6. A fresh clone has truthful bootstrap/degraded-mode behavior.
7. One dogfood scenario proves the packet improves agent work.

Everything else moves to later versions.

## 2. Explicit Deferrals

The following are valuable, but not required for v2.0:

### v2.1 candidates

- full memory ledger v2
- `layers memory list/search/show/retire/audit`
- memory conflict detection
- dedicated `layers-impact` crate
- richer GitNexus/git fallback impact engine

### v2.2 candidates

- session ledger schema
- Claude/Hermes/Codex importers
- `layers distill-session`
- draft memory accept/reject workflow
- packet quality scorer and benchmark suite

### v2.3 candidates

- autoresearch network fetchers
- autoresearch freshness scheduler
- release packaging polish beyond documented source install
- broader integration guides and examples

Do not pull deferred work into v2.0 unless it directly blocks the minimal compiler/MCP release.

## 3. Non-Goals for v2.0

Do not build or expand:

- general agent runtime
- chat product
- messaging gateway
- provider abstraction platform
- subagent orchestration
- hosted memory service
- generic vector database
- generic process/filesystem MCP tool server
- autonomous daemon-first UX

A v2.0 feature must do one of these:

1. feed a `ContextPacket`
2. compile/finalize a `ContextPacket`
3. render/validate/diff a `ContextPacket`
4. store minimal durable local context needed by packets
5. expose stable context tools to agents

## 4. Use Layers To Build Layers

Before each phase, run a self-preflight packet and use it as implementation context:

```bash
cargo run -- preflight \
  "Implement <phase objective> for Layers 2.0" \
  --target <primary files> \
  --agent-prompt \
  --no-audit
```

Rules:

- Save reviewed dogfood packets only under `docs/dogfood/`.
- Do not commit `.hermes/`, `memoryport/*.sqlite`, or telemetry churn.
- Promote durable findings with canonical memory rather than generated runtime state.
- If the packet misses important context, file that as a Layers product gap.

## 5. Global Gates

Every phase must pass:

```bash
cargo fmt --all --check
git diff --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Run full tests before phase completion:

```bash
cargo test --workspace --all-targets
```

Stable-core boundary must stay green after Phase 0:

```bash
cargo check --no-default-features --all-targets
cargo clippy --no-default-features --all-targets -- -D warnings
```

---

# Phase 0 — Boundary, Bootstrap, and CI Guardrails

**Objective:** Make the product boundary and build boundary enforceable before adding new v2.0 surface.

## Task 0.1: Product contract doc

**Objective:** Create a binding v2.0 contract that separates stable core from deprecated runtime compatibility.

**Files:**
- Create: `docs/V2_PRODUCT_CONTRACT.md`
- Modify: `README.md`
- Modify: `docs/NORTH_STAR.md`

**Steps:**
1. Run preflight:
   ```bash
   cargo run -- preflight \
     "Write the Layers v2 product contract" \
     --target README.md \
     --target docs/NORTH_STAR.md \
     --agent-prompt \
     --no-audit
   ```
2. Create `docs/V2_PRODUCT_CONTRACT.md` with sections:
   - stable core
   - beta/support
   - deprecated compatibility
   - non-goals
   - allowed feature jobs
   - v2.0 release boundary
3. Link it from `README.md` and `docs/NORTH_STAR.md`.
4. Verify markdown by reading the changed files.
5. Run:
   ```bash
   cargo fmt --all --check
   git diff --check
   ```

**Acceptance:** A new contributor can identify whether a proposed feature belongs in v2.0 stable core.

## Task 0.2: Stable-core CI gate

**Objective:** Prevent daemon/runtime dependencies from leaking into stable-core builds.

**Files:**
- Modify or create: `.github/workflows/ci.yml`
- Modify: `docs/development.md`

**Steps:**
1. Inspect existing workflows.
2. Add a CI job that runs:
   ```bash
   cargo check --no-default-features --all-targets
   cargo clippy --no-default-features --all-targets -- -D warnings
   ```
3. Document the gate in `docs/development.md`.
4. Verify locally with the same commands.

**Acceptance:** CI proves the stable target set can build without deprecated runtime or compatibility-storage features.

## Task 0.3: Resolve or isolate the `substrate` compatibility dependency

**Objective:** A fresh clone must have truthful, reproducible stable-core setup.

**Files:**
- Modify: `Cargo.toml`
- Modify: `README.md`
- Modify: `docs/development.md`
- Optionally modify: `.github/workflows/ci.yml`

**Decision order:**
1. Prefer a git or crates.io dependency for `substrate`.
2. If not possible, vendor or workspace it.
3. If neither is safe, feature-gate it away from stable core and document the compatibility path.

**Steps:**
1. Run:
   ```bash
   cargo tree -p layers --no-default-features
   ```
2. Identify whether stable core needs `substrate`, if it does.
3. Make the smallest dependency change that keeps stable core reproducible.
4. Update docs with exact setup commands.
5. Verify:
   ```bash
   cargo check --no-default-features --all-targets
   cargo clippy --no-default-features --all-targets -- -D warnings
   cargo check --workspace --all-targets
   ```

**Acceptance:** The stable-core path no longer depends on an undocumented sibling repo.

---

# Phase 1 — Minimal ContextPacket v2

**Objective:** Define and verify the stable packet artifact without overdesigning future memory/session/quality systems.

## Task 1.1: Add minimal v2 packet compatibility tests

**Objective:** Lock the v2 schema contract before changing implementation.

**Files:**
- Modify: `crates/layers-core/src/context_packet.rs`
- Create: `docs/examples/context-packet-v2-minimal.json`
- Create: `docs/schemas/context-packet-v2.md`

**Required v2 fields:**
- `schema_version`
- `id`
- `workspace_id`
- `query`
- `created_at`
- `git_ref`
- `route`
- `confidence`
- `budget`
- `sections`
- `warnings`
- `selection_trace`
- `retrieval`
- `provenance`

**TDD Steps:**
1. Add a failing test in `crates/layers-core/src/context_packet.rs` for v2 serde roundtrip.
2. Add a failing test that every `ContextItem` has source and selected reason.
3. Run:
   ```bash
   cargo test -p layers-core context_packet -- --nocapture
   ```
   Expected: fail for missing v2 fields.
4. Add minimal v2 fields and defaults.
5. Re-run the same test. Expected: pass.
6. Add `docs/examples/context-packet-v2-minimal.json` from a real serialized packet shape.
7. Document field meanings in `docs/schemas/context-packet-v2.md`.

**Acceptance:** `ContextPacket` has a clear v2 core without pulling in quality scoring, session schema, or memory ledger v2.

## Task 1.2: Add packet provenance only

**Objective:** Every packet can explain how it was compiled.

**Files:**
- Modify: `crates/layers-core/src/context_packet.rs`
- Modify: `src/context_packet_compiler/mod.rs`

**Minimal provenance fields:**
- compiler name/version
- command or surface: query/preflight/mcp
- workspace id
- git ref
- generated_at
- source adapter labels, e.g. workspace, memory, gitnexus, autoresearch

**Steps:**
1. Add failing tests that finalized packets include provenance.
2. Implement `PacketProvenance` with defaults.
3. Populate it in existing packet finalization.
4. Verify query and preflight JSON still parse:
   ```bash
   cargo run -- query "What should I know before changing README?" --json --no-audit | python3 -m json.tool >/dev/null
   cargo run -- preflight "Change README" --target README.md --json --no-audit | python3 -m json.tool >/dev/null
   ```

**Acceptance:** Provenance is stable and populated, but packet quality scoring is deferred.

---

# Phase 2 — `layers-compiler` Crate Extraction

**Objective:** Make one shared compiler engine the semantic source of truth for CLI and MCP.

## Task 2.1: Create the crate with copied pure helpers

**Objective:** Establish `layers-compiler` without changing command behavior.

**Files:**
- Create: `crates/layers-compiler/Cargo.toml`
- Create: `crates/layers-compiler/src/lib.rs`
- Modify: root `Cargo.toml`
- Modify: `src/context_packet_compiler/mod.rs`

**Steps:**
1. Run preflight:
   ```bash
   cargo run -- preflight \
     "Extract pure ContextPacket compiler helpers into crates/layers-compiler" \
     --target src/context_packet_compiler \
     --target crates/layers-core/src/context_packet.rs \
     --agent-prompt \
     --no-audit
   ```
2. Create `crates/layers-compiler` depending on `layers-core` and minimal shared dependencies.
3. Move pure helpers first: cited item construction, source helpers, evidence rendering, finalization helpers.
4. Leave side effects in the binary crate: git commands, audit writes, telemetry, printing.
5. Keep a temporary compatibility module in `src/context_packet_compiler/mod.rs` that re-exports moved helpers.
6. Verify:
   ```bash
   cargo test -p layers-compiler
   cargo test -q query -- --nocapture
   cargo test -q preflight -- --nocapture
   cargo clippy --bin layers -- -D warnings
   ```

**Acceptance:** No output behavior changes; pure packet helpers live in `layers-compiler`.

## Task 2.2: Add `ContextCompiler` request API

**Objective:** Introduce the API that CLI and MCP will share.

**Files:**
- Modify: `crates/layers-compiler/src/lib.rs`

**Initial API:**
```rust
pub struct ContextCompiler;

pub struct CompileRequest {
    pub task: String,
    pub targets: Vec<String>,
    pub mode: CompileMode,
    pub budget_words: usize,
}

pub enum CompileMode {
    Query,
    Preflight,
}
```

Do not include impact/session/autoresearch-specific request types yet unless existing packet compilation requires them.

**Steps:**
1. Add failing unit tests for constructing a minimal packet from `CompileRequest`.
2. Implement a minimal compiler that can produce an empty-but-valid packet.
3. Add extension points for workspace/memory/impact adapters, but keep implementations minimal.
4. Verify `cargo test -p layers-compiler`.

**Acceptance:** There is one typed compiler API, but command migration can happen incrementally.

## Task 2.3: Route `preflight` through `ContextCompiler`

**Objective:** Migrate the more packet-native command first.

**Files:**
- Modify: `src/cmd/preflight.rs`
- Modify: `crates/layers-compiler/src/lib.rs`

**Steps:**
1. Add/confirm tests for preflight JSON shape and agent-prompt rendering.
2. Move preflight packet assembly into `ContextCompiler` or compiler adapters.
3. Keep CLI parsing and output printing in `src/cmd/preflight.rs`.
4. Verify:
   ```bash
   cargo test -q preflight -- --nocapture
   cargo run -- preflight "Change README" --target README.md --json --no-audit | python3 -m json.tool >/dev/null
   ```

**Acceptance:** `preflight` semantics are compiler-backed.

## Task 2.4: Route `query` through `ContextCompiler`

**Objective:** Remove semantic drift between query and preflight packet construction.

**Files:**
- Modify: `src/cmd/query.rs`
- Modify: `crates/layers-compiler/src/lib.rs`

**Steps:**
1. Add/confirm tests for query JSON compatibility, low-confidence fallback, and evidence rendering.
2. Move packet assembly into the compiler while leaving retrieval, audit, telemetry, and printing in query until safe to move.
3. Preserve legacy JSON fields and evidence affordances.
4. Verify:
   ```bash
   cargo test -q query -- --nocapture
   cargo test -p layers --test uc_semantic_retrieval_e2e -- --nocapture
   cargo run -- query "What did we decide about Layers scope?" --json --no-audit | python3 -m json.tool >/dev/null
   ```

**Acceptance:** `query` and `preflight` use one packet finalization path.

---

# Phase 3 — Packet CLI Surface

**Objective:** Make ContextPacket a first-class file/artifact users can validate, inspect, and render.

## Task 3.1: Add `layers packet validate`

**Files:**
- Create: `src/cmd/packet.rs`
- Modify: `src/cmd/mod.rs`
- Modify: `src/main.rs`

**Steps:**
1. Add failing tests using `docs/examples/context-packet-v2-minimal.json`.
2. Implement `PacketCommands::Validate { file }`.
3. Validate JSON parse, schema version, required fields, and source/selection coverage.
4. Verify invalid packet returns non-zero.

**Acceptance:** `layers packet validate docs/examples/context-packet-v2-minimal.json` succeeds.

## Task 3.2: Add `inspect` and `render`

**Files:**
- Modify: `src/cmd/packet.rs`
- Modify: `src/main.rs`

**Commands:**
- `layers packet inspect <file>`
- `layers packet render <file> --markdown`
- `layers packet render <file> --agent-prompt`

**Steps:**
1. Add tests for inspect summary.
2. Reuse existing `ContextPacket::to_markdown` and `to_agent_prompt`.
3. Verify outputs include query, route, warnings, sections, and sources.

**Acceptance:** Packet files are useful outside the original command invocation.

## Task 3.3: Add `diff` as minimal structural diff

**Files:**
- Modify: `src/cmd/packet.rs`

**Scope:**
- Compare schema version, query, route, warning count, section ids, item ids, and budget status.
- Do not build a full semantic diff engine in v2.0.

**Acceptance:** Users can see whether packet compilation materially changed between two runs.

---

# Phase 4 — Minimal Compiler-Backed MCP v2

**Objective:** Expose Layers to other agents through safe, stable, compiler-backed MCP tools.

## Task 4.1: Compiler-backed `context_compile` and `preflight_context`

**Files:**
- Modify: `crates/layers-mcp/src/stable.rs`
- Modify: `crates/layers-mcp/Cargo.toml`

**Steps:**
1. Add dependency on `layers-compiler` if needed.
2. Replace placeholder packet generation with `ContextCompiler` calls.
3. Add a `preflight_context` tool if not present.
4. Add tests proving MCP output is valid `ContextPacket` JSON.

**Acceptance:** MCP and CLI use the same compiler path for packet generation.

## Task 4.2: Stable MCP allowlist safety tests

**Files:**
- Modify: `crates/layers-mcp/src/server.rs`
- Modify: `crates/layers-mcp/src/stable.rs`

**Assertions:**
1. Stable config exposes only stable context tools.
2. Runtime/process/filesystem/subagent tools are hidden by default.
3. Empty/default allowlists expose nothing.
4. Stable tool fixture dispatches successfully.

**Verify:**
```bash
cargo test -p layers-mcp stable -- --nocapture
cargo test -p layers-mcp -- --nocapture
cargo clippy -p layers-mcp --all-targets -- -D warnings
```

**Acceptance:** Other agents can safely connect without receiving generic runtime power.

## Task 4.3: Minimal packet validation MCP tool

**Files:**
- Modify: `crates/layers-mcp/src/stable.rs`

**Tool:**
- `packet_validate`

**Scope:**
- Accept packet JSON.
- Return structured validation result.
- Reuse packet validation logic from `layers packet validate` if possible.

**Acceptance:** MCP clients can validate packets without shelling out to CLI.

---

# Phase 5 — v2.0 Dogfood Proof

**Objective:** Prove the minimal compiler/MCP release improves real agent work before expanding scope.

## Task 5.1: Write dogfood protocol

**Files:**
- Create: `docs/dogfood/V2_MINIMAL_DOGFOOD_PROTOCOL.md`

**Protocol:**
1. Pick one real Layers issue.
2. Run `layers preflight` and save reviewed packet.
3. Give the packet to one coding agent.
4. Record whether it surfaced relevant memory, files, risks, and validation commands.
5. Record at least one missing-context finding.
6. Promote durable finding into memory or file a roadmap follow-up.

**Acceptance:** Dogfood produces evidence and at least one improvement loop.

## Task 5.2: Run one proof scenario

**Scenario:**
Use Layers to implement or review one v2.0 phase task.

**Files:**
- Create: `docs/dogfood/<timestamp>-v2-minimal-proof/`

**Artifacts:**
- preflight packet JSON or agent prompt
- task outcome summary
- missing-context notes
- verification output

**Acceptance:** v2.0 has one real proof, not just architecture claims.

---

# Phase 6 — Documentation and Release Readiness

**Objective:** Make the minimal v2.0 usable by someone other than the author.

## Task 6.1: Update README around v2.0 minimal workflow

**Files:**
- Modify: `README.md`
- Modify: `docs/cli.md`
- Modify: `docs/walkthrough.md`

**Required examples:**
```bash
layers preflight "Refactor auth middleware" --target src/auth --agent-prompt
layers query "What did we decide about model routing?" --json
layers packet validate packet.json
layers mcp serve
```

**Acceptance:** README teaches the v2.0 core path without emphasizing deprecated runtime surfaces.

## Task 6.2: Add integration notes for generic MCP and one agent

**Files:**
- Create: `docs/integrations/mcp.md`
- Create one of:
  - `docs/integrations/claude-code.md`
  - `docs/integrations/codex.md`
  - `docs/integrations/hermes.md`

**Acceptance:** At least one real agent can be wired to Layers without guessing.

## Task 6.3: Release checklist

**Files:**
- Create: `docs/release-v2.0-checklist.md`

**Checklist:**
- stable-core check passes
- full workspace tests/check/clippy/fmt pass
- packet example validates
- CLI docs match help output
- MCP stable tool list verified
- bootstrap documented
- dogfood proof exists
- deprecated surfaces labeled

**Acceptance:** Release readiness is auditable.

---

# Layers 2.0 Release Definition

Layers 2.0 is ready when all are true:

- Product contract is documented.
- Fresh clone bootstrap is truthful and reproducible for stable core.
- Stable-core no-default-feature check and clippy pass.
- `ContextPacket` v2 minimal schema exists with docs and example.
- `query` and `preflight` compile packets through `layers-compiler`.
- `layers packet validate/inspect/render/diff` works.
- MCP exposes compiler-backed stable context tools only by default.
- Full workspace fmt/check/test/clippy gates pass.
- README and CLI docs teach the stable core path.
- One dogfood proof shows a packet improving or materially guiding agent work.

# Execute First

Start with Phase 0 only.

Do not begin memory ledger v2, session import, packet quality scoring, autoresearch network fetching, or release packaging until Phase 0 through Phase 4 are complete and verified.

The v2.0 project succeeds or fails on the trustworthiness of the packet/compiler/MCP core. Everything else is v2.x expansion.
