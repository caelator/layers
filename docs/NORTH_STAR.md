# Layers North Star

## North Star

Layers is the local-first context compiler for coding agents.

It turns repository structure, Git history, code graph intelligence, project memory, previous agent sessions, decisions, constraints, failures, and plans into bounded, cited, reproducible context packets that any agent can consume before it edits.

Layers should make this question easy to answer:

> What does this agent need to know before touching this code?

## Product Promise

Given a task and a workspace, Layers produces an auditable context packet containing:

- relevant project memories
- active constraints and prior decisions
- known failures and pitfalls
- relevant files, symbols, and execution flows
- GitNexus impact analysis when available
- suggested validation commands
- source citations and selection reasons
- warnings when context is degraded, stale, conflicting, or incomplete

The packet must be useful to humans and directly injectable into agents such as Hermes, Claude Code, Codex, Gemini, OpenClaw, DeerFlow, and Letta.

## What Layers Is

Layers is:

- a local-first Rust CLI
- a context packet generator
- an explicit project memory ledger
- a Git/code-graph adapter around GitNexus
- a bridge between agent sessions and durable project memory
- an MCP/CLI surface other agents can call
- an audit-friendly way to explain why context was selected

## What Layers Is Not

Layers is not:

- a personal assistant
- a chat product
- a hosted service
- a full agent runtime
- a messaging gateway
- a generic tool-execution framework
- a generic vector database
- a generic temporal knowledge graph
- a replacement for Hermes, OpenClaw, DeerFlow, Letta, mem0, Graphiti, Cognee, or MemoryPort

Those systems should be treated as execution layers, memory backends, or integration peers. Layers should make them better by giving them reliable coding context.

## Strategic Boundary

Layers owns context assembly, not task execution. The v2 stable-core contract is defined in [V2 Product Contract](V2_PRODUCT_CONTRACT.md).

Stable Layers work should serve one of these jobs:

1. assemble context before a code change
2. preserve useful project memory after work completes
3. import/distill agent sessions into explicit memory
4. expose memory and impact context to other agents
5. verify that local context dependencies are healthy

Work outside those jobs is non-essential and should be experimental, deprecated, or moved out of the core path.

## Stable Core

The v2.0 stable surface is defined by [V2 Product Contract](V2_PRODUCT_CONTRACT.md). Longer-term v2.x work should converge on:

- `layers query` / `layers preflight` — produce a context packet
- `layers packet validate` — validate persisted context packet artifacts
- `layers packet inspect` — summarize packet provenance, warnings, sections, and quality signals
- `layers packet render` — render persisted packets for human review, agent prompts, JSON normalization, or objective briefs
- `layers packet diff` — compare persisted packets as reviewable, body-safe artifacts
- `layers remember` — write explicit project memory
- `layers memory` — list/search/show/retire/audit project memory
- `layers impact` — summarize blast radius for a file/symbol/task
- `layers import-session` — normalize agent sessions into a local event ledger
- `layers distill-session` — draft memory from a session
- `layers promote` — turn draft/council/session outputs into canonical memory
- `layers validate` / `layers doctor` — explain local readiness and degraded modes
- `layers refresh` — update GitNexus/MemoryPort derived context
- `layers mcp serve` — expose the stable core to other agents

## Beta Surface

Beta features are useful when they feed context packet generation, memory quality, or local context dependency health, but should not define the product:

- council deliberation
- council promotion
- route-quality benchmarks
- context-quality evaluation
- technician/health automation when scoped to context dependencies

## Deprecated / Non-Essential Surface

These features are deprecated as core product direction. They may remain for experiments, but new work should not expand them unless they directly support context packet generation:

- general chat loop
- web portal as primary UX
- daemon as primary runtime
- model provider abstraction as a platform
- subagent execution framework
- messaging channels
- generic tool runtime
- infrastructure credential management
- autonomous monitor/fixer workflows

## Design Principles

1. Local-first by default.
2. Explicit memory beats opaque background state.
3. Every context item needs provenance.
4. Reproducible packets beat clever hidden retrieval.
5. Degraded context is acceptable if clearly labeled.
6. Agent-neutral interfaces beat one-off integrations.
7. Git/code semantics matter more than generic document memory.
8. Do not duplicate mature agent frameworks.
9. Keep the stable core small enough to trust.
10. Optimize for usefulness before breadth.

## Success Metrics

Layers is succeeding when:

- a fresh agent can start useful work faster after reading a Layers packet
- fewer bugs come from forgotten project constraints
- previous failed attempts are surfaced before repetition
- context packets cite their sources and selection reasons
- Hermes/Claude/Codex/OpenClaw can call Layers without special setup beyond CLI/MCP
- a fresh clone can run the stable core with clear degraded-mode behavior
- non-essential runtime features are not required for daily value

## One-Line Positioning

Layers is the local-first context spine for coding agents: it compiles project memory, Git/code intelligence, and prior sessions into auditable context packets before agents act.
