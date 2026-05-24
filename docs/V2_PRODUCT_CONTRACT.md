# Layers v2 Product Contract

## Binding Product Definition

Layers v2 is the local-first `ContextPacket` compiler for coding agents.

Its stable job is to answer:

> What does this agent need to know before touching this code?

The answer must be a bounded, cited, reproducible `ContextPacket` that can be consumed by humans, CLI workflows, MCP clients, and coding agents before they edit a repository.

## Core Artifact

`ContextPacket` is the only core product artifact.

A stable v2 packet must preserve:

- task/query text
- workspace identity
- git/worktree state when available
- ordered context sections
- source citations
- selection reasons
- warnings for degraded, stale, incomplete, or conflicting context
- validation guidance when available
- machine-readable JSON rendering
- human-readable Markdown or agent-prompt rendering

Commands, stores, adapters, and MCP tools are stable only insofar as they feed, compile, validate, render, or expose this artifact.

## Allowed Stable-Core Feature Jobs

A v2 feature belongs in stable core only if it does at least one of these jobs:

1. feed a `ContextPacket`
2. compile or finalize a `ContextPacket`
3. render, validate, inspect, or diff a `ContextPacket`
4. store minimal durable local context needed by future packets
5. expose stable context tools to agents through CLI or MCP

If a feature does not satisfy one of these jobs, it must be beta, deprecated compatibility, or out of scope.

## Non-Goals

Layers v2.0 is not:

- a personal assistant
- a chat-first product
- a hosted service
- a general agent runtime
- a messaging gateway
- a provider abstraction platform
- a subagent orchestrator
- a generic tool execution server
- a generic vector database
- a replacement for Hermes, OpenClaw, DeerFlow, Letta, mem0, Graphiti, Cognee, or MemoryPort

Those systems are execution layers, memory backends, or integration peers. Layers should make them better by compiling local coding context.

## Removed Surfaces

No existing user-facing command is removed in v2.0 solely because of this contract.

Instead, v2.0 distinguishes stable core from deprecated compatibility. Deprecated surfaces may remain available behind default compatibility features, but they are not allowed to block stable-core no-default-feature builds or appear in stable MCP defaults. Future major releases may remove compatibility surfaces after separate migration plans.

## Stable Core Surface

The v2.0 stable core converges on:

| Surface | Status | Contract |
|---------|--------|----------|
| `ContextPacket` schema/renderers | Stable core | Versioned context artifact and renderings |
| `layers query` | Stable core | Compile task context from local memory/retrieval inputs |
| `layers preflight` | Stable core | Compile pre-edit context for code-heavy work and explicit targets |
| `layers packet validate/inspect/render/diff/grade` | Stable core | Treat packets as durable artifacts |
| `layers validate` / `layers doctor` | Stable core | Explain readiness and degraded modes |
| `layers refresh` | Support | Refresh optional derived context such as GitNexus data |
| `layers mcp serve` stable tools | Stable core | Expose context compilation and packet validation to agents |
| `layers memory list/search/show` | Stable core | Inspect existing curated/remembered memory without a new backend |
| `layers impact <target>` | Stable core | Summarize GitNexus-backed or degraded blast-radius context |

## Beta / v2.x Expansion Surface

These are strategically aligned, but not required to ship the minimal v2.0 release:

| Surface | Target | Reason |
|---------|--------|--------|
| session import/distillation | v2.2 | Cross-agent continuity from transcripts |
| packet quality scoring | v2.2 | Measurable packet usefulness after dogfood evidence |
| autoresearch freshness/network fetchers | v2.3 | External context intake once compiler core is stable |
| release packaging | v2.x | Distribution polish after core semantics settle |

## Deprecated Compatibility Surface

These surfaces may remain available for compatibility or experiments, but must not drive v2 architecture:

| Surface | Status | Rule |
|---------|--------|------|
| `layers daemon` | Deprecated / compatibility | Default-compatible, not stable-core required |
| `layers chat` | Deprecated / experimental | Must not become primary UX |
| monitor / technician automation | Deprecated / experimental | Only useful when scoped to context health |
| provider runtime | Deprecated / compatibility | Do not expand as a platform |
| messaging channels | Deprecated / compatibility | Do not expand as product surface |
| generic tool runtime | Deprecated / compatibility | Do not expose through stable MCP defaults |
| subagent orchestration | Deprecated / compatibility | Do not compete with mature agent frameworks |
| infrastructure credential management | Deprecated / experimental | Outside context compiler thesis |

## Stable-Core Build Boundary

The stable target set must continue to compile without deprecated runtime and compatibility-storage defaults:

```bash
cargo check --no-default-features --all-targets
cargo clippy --no-default-features --all-targets -- -D warnings
```

Default features may preserve compatibility, but no-default stable-core checks are the guardrail against accidental runtime or storage gravity.

## Stable MCP Contract

Stable MCP defaults must expose only product-facing context tools. They must not expose generic filesystem, process, daemon, subagent, or mutating runtime capabilities.

Allowed stable MCP tool classes:

- context compilation
- preflight context compilation
- packet validation
- read-only memory retrieval where implemented
- read-only impact/context analysis where implemented

Generic runtime tools require explicit opt-in outside the stable context surface.

## Release Boundary for v2.0

Layers v2.0 is complete when:

- `ContextPacket` v2 minimal schema is documented and tested
- `query` and `preflight` compile packets through a shared compiler path
- packet validate/inspect/render/diff/grade commands exist
- stable MCP context tools call the shared compiler path
- stable-core no-default-feature checks pass in CI
- fresh clone bootstrap is truthful and reproducible for stable core
- at least one dogfood proof demonstrates packet-guided agent work

Memory ledger v2, session distillation, impact engine v2, packet quality benchmarks, and autoresearch network fetchers are v2.x expansion, not v2.0 blockers.
