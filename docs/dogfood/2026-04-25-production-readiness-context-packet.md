# Dogfood Context Packet — Production Readiness Reset

Date: 2026-04-25
Workspace: `/Users/xxx/layers`
Task: Make Layers production-ready and useful as a local-first context compiler for coding agents.

## Query

What context matters before making Layers production-ready and dogfooding it?

## Selected Context

### North Star

Source: `docs/NORTH_STAR.md`

Layers is now scoped as a local-first context compiler for coding agents. It should assemble project memory, Git/code intelligence, and prior sessions into auditable context packets before agents act.

Selection reason: direct product direction.

### v0.2 Roadmap

Source: `docs/ROADMAP_v0.2.md`

The near-term roadmap is a scope reset around bootstrap truth, ContextPacket v1, explicit memory, Git-aware impact, session import/distillation, MCP, context quality evaluation, and integrations.

Selection reason: direct roadmap input.

### Existing Curated Memory: Structured Records Are Canonical

Source: `memoryport/curated-memory.jsonl`
Record: `cm_decision_layers_canonical-curated-memories-should-be-structured-records-not-vectors-as-source-of-truth-semantic-embeddings-are-derived-retrieval-artifacts`

Canonical curated memories should be structured records, not vectors as source of truth. Semantic embeddings are derived retrieval artifacts.

Selection reason: constrains memory architecture.

### Existing Curated Memory: GitNexus Is First-Class

Source: `memoryport/curated-memory.jsonl`
Record: `cm_decision_layers_gitnexus-is-a-first-class-structural-provider-and-should-evolve-from-ad-hoc-graph-queries-toward-workflow-operational-use-in-planning-review-handoff-and-postmortem-packets`

GitNexus is a first-class structural provider and should evolve toward operational use in planning, review, handoff, and postmortem packets.

Selection reason: supports impact/context packet differentiation.

### Existing Curated Memory: Rust Constraint

Source: `memoryport/curated-memory.jsonl`
Record: `cm_constraint_layers_prefer-rust-for-durable-systems-use-scripting-only-for-glue-or-temporary-scaffolding`

Prefer Rust for durable systems; use scripting only for glue or temporary scaffolding.

Selection reason: constrains implementation strategy.

### Bootstrap Blocker

Source: local command result
Command: `cargo test --quiet`

Failure:

```text
failed to read /Users/xxx/substrate/Cargo.toml
```

The root `Cargo.toml` depends on `substrate = { path = "../substrate" }`, but `/Users/xxx/substrate` is missing locally.

Selection reason: production-readiness blocker.

### Deprecated Surface Decision

Source: `README.md`, `docs/cli.md`, `src/main.rs`

Non-essential features are now deprecated as core direction:

- chat
- daemon-first UX
- portal
- generic provider runtime
- generic tool runtime
- subagents
- messaging channels
- infrastructure credential manager
- autonomous monitor/fixer

Selection reason: prevents product sprawl.

## Warnings

1. Build/test validation is blocked until `../substrate` is resolved.
2. Current dogfood is manual because the stable ContextPacket v1 command does not exist yet.
3. Several deprecated runtime surfaces still exist in code; docs and CLI labels deprecate direction but do not remove behavior.
4. Production readiness should start with bootstrap/build truth before new features.

## Recommended Next Steps

1. Resolve or feature-gate the `../substrate` dependency.
2. Add `scripts/bootstrap.sh` and/or `layers doctor`.
3. Implement ContextPacket v1 types and renderers.
4. Convert `layers query --json` to ContextPacket v1.
5. Add `layers memory` inspection commands.
6. Add `layers impact` with GitNexus fallback.
7. Add Hermes session importer and distillation drafts.
8. Expose stable MCP tools.
9. Add context-quality fixtures from this dogfood run.

## Dogfood Assessment

This packet is manually assembled but follows the intended Layers output shape:

- task/query
- selected context
- source citations
- selection reasons
- warnings
- next steps

Missing from current product:

- typed packet schema
- automatic source ranking
- automatic token budgeting
- first-class warning model
- context-quality eval fixture generation

Promote as memory:

- Layers production readiness is blocked by hidden `../substrate` dependency.
- Layers dogfood artifacts must be created for each milestone.
- ContextPacket v1 must be implemented before broadening any runtime features.
