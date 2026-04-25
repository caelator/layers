# Layers Production Readiness and Dogfood Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Make Layers production-ready as a local-first context compiler for coding agents, then dogfood it on Layers, Hermes Agent, and Triumvirate.

**Architecture:** Keep the stable core small: context packets, explicit memory, Git-aware impact context, session import/distillation, and MCP access. Move non-essential agent-runtime surfaces behind experimental status and prevent them from driving the product architecture.

**Tech Stack:** Rust 2024, Cargo workspace, Clap, Serde/JSONL, GitNexus CLI/MCP, MemoryPort/uc, optional LanceDB, MCP, shell-based integration tests.

---

## Production Readiness Definition

Layers is production-ready when a fresh user can:

1. Clone the repo and build the stable core from documented steps.
2. Run `layers doctor` or `layers validate` and understand exactly which capabilities are available or degraded.
3. Run `layers query <task> --json` and receive a stable ContextPacket v1 with citations and selection reasons.
4. Add and inspect explicit project memory.
5. Ask for Git-aware impact context before editing.
6. Import at least one prior agent session and distill it into draft memories.
7. Expose the stable core through MCP to Hermes or another agent.
8. Run a documented dogfood workflow that proves Layers improves real coding-agent work.

Non-goals for production readiness:

- productionizing the chat portal
- productionizing the daemon as the primary UX
- productionizing messaging channels
- productionizing a generic model-provider runtime
- competing with Hermes/OpenClaw/DeerFlow as an agent framework

---

## Dogfood Rules

Every production-readiness milestone must produce at least one durable dogfood artifact:

- a context packet
- a memory record
- an impact packet
- a session distillation
- an MCP call transcript
- an eval fixture
- a postmortem

Dogfood artifacts live under:

```text
docs/dogfood/
```

Canonical lessons promoted from dogfood should be written to:

```text
memoryport/curated-memory.jsonl
```

Dogfood must use the same workflow that Layers is supposed to enable:

1. Ask for context before work.
2. Do the work.
3. Record what changed.
4. Promote durable lessons.
5. Evaluate whether the context helped.

---

## Milestone 0: Bootstrap Truth and Build Health

**Objective:** Remove hidden setup traps so the stable core can be built and verified.

### Task 0.1: Document the current substrate blocker as a failing production-readiness check

**Files:**
- Modify: `docs/development.md`
- Modify: `docs/PRODUCTION_READINESS_DOGFOOD_PLAN.md`
- Dogfood: `docs/dogfood/2026-04-25-bootstrap-context-packet.md`

**Steps:**
1. Record that `cargo test` fails without `../substrate`.
2. Add a dogfood context packet explaining the blocker.
3. Add a curated memory record of type `failure` or `constraint`.

**Verify:**

```bash
git diff --check
```

Expected: pass.

### Task 0.2: Resolve `../substrate` dependency strategy

**Objective:** Decide whether `substrate` is vendored, published, git-sourced, optional, or bootstrapped.

**Files:**
- Modify: `Cargo.toml`
- Possibly create: `scripts/bootstrap.sh`
- Modify: `README.md`
- Modify: `docs/development.md`

**Preferred implementation order:**
1. If `caelator/substrate` exists and is intended public dependency, switch to a git dependency or add bootstrap script.
2. If it is core and small enough, move it into `crates/substrate` and include it in the workspace.
3. If it is only needed by deprecated/experimental runtime code, feature-gate the dependent code.

**Acceptance:**

A fresh clone no longer fails with:

```text
failed to read ../substrate/Cargo.toml
```

**Verify:**

```bash
cargo metadata --no-deps
cargo test --quiet
```

Expected: both pass or fail only on real tests, not missing dependency metadata.

### Task 0.3: Add `scripts/bootstrap.sh`

**Objective:** Provide one command that tells a user what they can run.

**Files:**
- Create: `scripts/bootstrap.sh`
- Modify: `README.md`
- Modify: `docs/development.md`

**Behavior:**

The script checks:

- Rust version >= 1.85
- Cargo metadata works
- GitNexus availability
- `uc` availability
- `~/.memoryport/uc.toml` availability
- optional model CLIs: `claude`, `codex`, `gemini`
- whether stable core commands are expected to work

**Verify:**

```bash
bash scripts/bootstrap.sh
```

Expected: clear pass/degraded/fail matrix.

---

## Milestone 1: Command Surface Stabilization

**Objective:** Make deprecated surfaces visible and prevent accidental expansion.

### Task 1.1: Add stability labels to CLI help

**Files:**
- Modify: `src/main.rs`
- Test: existing CLI tests or new CLI snapshot tests if available

**Required labels:**

- `[stable core]`
- `[support]`
- `[beta]`
- `[deprecated/experimental]`

**Verify:**

```bash
cargo run -- --help
```

Expected: command help shows status labels.

### Task 1.2: Add architectural guardrail doc

**Files:**
- Create or update: `docs/NORTH_STAR.md`
- Modify: `AGENTS.md` only if adding a short pointer is acceptable

**Rule:**

Any new feature must answer at least one of:

1. Does it improve context packet quality?
2. Does it preserve explicit project memory?
3. Does it import/distill prior agent sessions?
4. Does it expose context/memory/impact to other agents?
5. Does it improve readiness/degraded-mode diagnostics?

**Verify:**

Manual review.

---

## Milestone 2: ContextPacket v1

**Objective:** Make the context packet a stable, typed artifact.

### Task 2.1: Add `ContextPacket` core types

**Files:**
- Create: `crates/layers-core/src/context_packet.rs`
- Modify: `crates/layers-core/src/lib.rs`
- Test: module tests in `context_packet.rs`

**Types:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextPacket {
    pub schema_version: u32,
    pub id: String,
    pub workspace_id: String,
    pub query: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub git_ref: Option<String>,
    pub budget: ContextBudget,
    pub sections: Vec<ContextSection>,
    pub warnings: Vec<ContextWarning>,
    pub selection_trace: Vec<SelectionTraceEntry>,
}
```

Also define:

- `ContextBudget`
- `ContextSection`
- `ContextItem`
- `ContextSource`
- `ContextWarning`
- `SelectionTraceEntry`

**Test first:**

Write a serde roundtrip test before implementation.

**Verify:**

```bash
cargo test -p layers-core context_packet --quiet
```

Expected: pass.

### Task 2.2: Add renderers

**Files:**
- Create: `crates/layers-core/src/context_render.rs`
- Modify: `crates/layers-core/src/lib.rs`
- Test: renderer snapshot/unit tests

**Renderers:**

- JSON via serde
- Markdown
- agent prompt

**Verify:**

```bash
cargo test -p layers-core context_render --quiet
```

Expected: pass.

### Task 2.3: Convert `layers query --json` to ContextPacket v1

**Files:**
- Modify: `src/cmd/query.rs`
- Modify: `src/types.rs` if needed
- Modify: `src/memory.rs`
- Modify: `src/graph.rs`
- Test: query JSON test

**Behavior:**

`layers query "..." --json` returns a `ContextPacket` with:

- schema_version = 1
- memory section
- graph section when available
- warning section when providers unavailable
- selection trace entries

**Verify:**

```bash
cargo run -- query "What should I know before editing src/main.rs?" --json | jq .schema_version
```

Expected: `1`.

### Task 2.4: Add `--agent-prompt`

**Files:**
- Modify: `src/main.rs`
- Modify: `src/cmd/query.rs`
- Test: CLI/render test

**Behavior:**

```bash
layers query "..." --agent-prompt
```

prints direct prompt text suitable for Hermes/Claude/Codex.

**Verify:**

Output includes:

- task
- warnings
- selected context
- validation suggestions
- citations

---

## Milestone 3: Explicit Memory v1

**Objective:** Make local memory inspectable and maintainable.

### Task 3.1: Add normalized memory record types

**Files:**
- Create or modify: `src/memory/record.rs` or existing memory module
- Test: record serde tests

**Record kinds:**

- decision
- constraint
- failure
- plan
- status
- test_command
- architecture_note
- open_question
- handoff
- agent_observation

**Fields:**

- id
- kind
- project
- summary
- body/rationale/impact
- tags
- source
- status: active/superseded/retired
- confidence
- created_at
- updated_at
- supersedes
- linked_files
- linked_symbols

### Task 3.2: Add `layers memory list/search/show/retire/audit`

**Files:**
- Create: `src/cmd/memory.rs`
- Modify: `src/main.rs`
- Modify: `docs/cli.md`
- Tests: command-level tests where practical

**Verify:**

```bash
layers memory list
layers memory search "DeepSeek"
layers memory show <id>
layers memory audit
```

Expected: deterministic human output and JSON option later.

### Task 3.3: Add conflict detection basics

**Files:**
- Modify: memory module
- Test: conflict unit tests

**Initial conflict rules:**

- two active records with same normalized title but different body
- active record superseded by another active record but not marked superseded
- active test commands for same scope with different commands

---

## Milestone 4: Git-Aware Impact Context

**Objective:** Make Layers valuable before code edits.

### Task 4.1: Add `layers impact <target>`

**Files:**
- Create: `src/cmd/impact.rs`
- Modify: `src/main.rs`
- Modify: `src/graph.rs`
- Test: graph adapter/fallback tests

**Behavior:**

Return ContextPacket sections for:

- direct callers/dependents
- affected execution flows
- related files
- recent commits
- linked memories
- risk level
- validation suggestions

### Task 4.2: Degraded fallback without GitNexus

**Behavior:**

If GitNexus is unavailable:

- warn clearly
- use file search/Git history fallback
- still return a packet

**Verify:**

Run with PATH excluding GitNexus and ensure output is degraded, not fatal.

---

## Milestone 5: Session Ledger and Distillation

**Objective:** Turn prior agent work into reusable project context.

### Task 5.1: Define session event schema

**Files:**
- Create: `crates/layers-core/src/session_event.rs`
- Modify: `crates/layers-core/src/lib.rs`
- Test: serde roundtrip tests

**Events:**

- session_started
- user_message
- assistant_message
- tool_call
- tool_result
- file_edited
- command_run
- test_result
- decision_made
- failure_observed
- session_ended

### Task 5.2: Add Hermes importer

**Files:**
- Create: `src/cmd/import_session.rs`
- Modify: `src/main.rs`
- Test: fixture-based importer tests

**Input:**

Hermes session JSON/JSONL from `~/.hermes/sessions` or exported sessions.

**Output:**

Normalized session ledger under:

```text
memoryport/session-ledger/*.jsonl
```

### Task 5.3: Add distillation drafts

**Files:**
- Create: `src/cmd/distill_session.rs`
- Modify: memory module
- Tests: fixture-based distillation tests

**Output:**

Draft records under:

```text
memoryport/drafts/*.jsonl
```

No automatic promotion.

---

## Milestone 6: MCP Stable Core

**Objective:** Let Hermes and other agents call Layers.

### Task 6.1: Expose stable MCP tools

**Files:**
- Modify: `crates/layers-mcp/src/server.rs`
- Modify: `crates/layers-mcp/src/types.rs`
- Modify: `crates/layers-mcp/src/bridge.rs`
- Tests: MCP request/response tests

**Tools:**

- `layers_context_packet`
- `layers_remember`
- `layers_impact`
- `layers_memory_search`
- `layers_promote`
- `layers_doctor`

**Rule:**

Do not expose deprecated runtime surfaces by default.

### Task 6.2: Hermes dogfood integration

**Files:**
- Create: `docs/integrations/hermes.md`
- Dogfood artifact: `docs/dogfood/<date>-hermes-mcp-smoke.md`

**Acceptance:**

Hermes can request a context packet for a Layers task.

---

## Milestone 7: Context Quality Evaluation

**Objective:** Make context packet quality measurable.

### Task 7.1: Add benchmark fixture format

**Files:**
- Create: `benchmarks/context-packets/README.md`
- Create: `benchmarks/context-packets/layers-core.jsonl`

Each fixture includes:

- query
- expected memory IDs
- expected graph targets
- forbidden irrelevant IDs
- max token budget

### Task 7.2: Add `layers eval context`

**Files:**
- Create: `src/cmd/eval.rs`
- Modify: `src/main.rs`
- Tests: fixture test

**Metrics:**

- required memory recall
- required graph target recall
- forbidden item rate
- warnings emitted
- token budget utilization

---

## Milestone 8: Dogfood Campaign

**Objective:** Prove Layers is useful on real repositories.

### Repo 1: Layers itself

Tasks:

- Produce context packet for ContextPacket v1 implementation.
- Record bootstrap failure around substrate.
- Record deprecated surface decision.
- Use `layers impact` before changing query/render code once implemented.

### Repo 2: Hermes Agent

Tasks:

- Produce context packet for a provider/model metadata change.
- Compare with previous manual DeepSeek fix workflow.
- Record whether Layers surfaced useful gotchas.

### Repo 3: Triumvirate

Tasks:

- Produce context packet for a governance/protocol change.
- Confirm project north-star and constitutional constraints are surfaced.
- Record missing context as benchmark fixtures.

Dogfood reports:

```text
docs/dogfood/YYYY-MM-DD-layers-report.md
docs/dogfood/YYYY-MM-DD-hermes-report.md
docs/dogfood/YYYY-MM-DD-triumvirate-report.md
```

Each report includes:

- task
- context packet command
- what context helped
- what context was missing
- false positives
- memories promoted
- benchmark cases added

---

## Production Release Gates

v0.2 may be tagged only when:

1. Bootstrap works from a fresh clone.
2. Stable core builds and tests pass.
3. Deprecated commands are labeled in help/docs.
4. `layers query --json` emits ContextPacket v1.
5. `layers memory` can inspect and retire records.
6. `layers impact` works with GitNexus and degrades without it.
7. At least one Hermes/Claude/Codex integration can call Layers.
8. At least five context-quality fixtures exist.
9. Dogfood reports exist for Layers and one external repo.
10. README presents the narrowed product, not deprecated runtime surfaces.

## First Dogfood Packet: This Plan

This plan itself is the first dogfood artifact.

Task:

> Make Layers production-ready and useful as a context compiler.

Context used:

- `docs/NORTH_STAR.md`
- `docs/ROADMAP_v0.2.md`
- `README.md`
- `docs/cli.md`
- `docs/development.md`
- `memoryport/curated-memory.jsonl`
- current build failure: missing `../substrate`

Immediate promoted memories:

1. Layers is narrowed to context compiler, not agent runtime.
2. `../substrate` is a production-readiness blocker.
3. Dogfood must produce context packets, memory records, and eval fixtures.

## Commit Strategy

Use small commits:

1. `docs: add production readiness and dogfood plan`
2. `docs: record first dogfood context packet`
3. `chore: resolve substrate bootstrap blocker`
4. `feat: add context packet core types`
5. `feat: render query output as context packet v1`
6. `feat: add memory inspection commands`
7. `feat: add impact command`
8. `feat: add session import ledger`
9. `feat: expose stable layers mcp tools`
10. `test: add context quality benchmarks`

## Next Action

Start with Milestone 0. Do not implement ContextPacket until the build/bootstrap blocker is resolved or cleanly feature-gated.
