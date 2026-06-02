# Layers North Star

## North Star

Layers is the local-first context compiler for coding agents **and** an autospawned, continuously-chained research-and-implementation runtime on top of that compiler.

It turns repository structure, Git history, code graph intelligence, project memory, previous agent sessions, decisions, constraints, failures, and plans into bounded, cited, reproducible context packets that any agent can consume before it edits. It also runs autonomous research-and-implementation cycles on dedicated branches, chained back-to-back across runs with a configurable cooldown and a soft wall-clock cap, behind the same TDD + verification gate + `[verified]` commit discipline as a human in the loop.

Layers should make this question easy to answer:

> What does this agent need to know before touching this code?

And, when the user is away from the keyboard:

> What useful, evidence-gated work can an autospawned, continuously-chained research runtime do to this codebase, and what is the audit trail?

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
- an always-on agent runtime *on this machine's primary session or on protected branches* (Layers' research runtime is restricted to dedicated `autoresearch/<tag>` branches, runs behind a configurable cooldown and soft wall-clock cap, and stops on `SIGINT`/`SIGTERM` or unrecoverable error; it never holds the user's primary session, never touches `main`/`master`/release branches, and never competes with a human at the keyboard)
- a messaging gateway
- a generic tool-execution framework
- a generic vector database
- a generic temporal knowledge graph
- a replacement for Hermes, OpenClaw, DeerFlow, Letta, mem0, Graphiti, Cognee, or MemoryPort

Those systems should be treated as execution layers, memory backends, or integration peers. Layers should make them better by giving them reliable coding context, and — under the new research-runtime job — by running autospawned, continuously-chained cycles on dedicated branches that produce `[verified]` commits and a reviewable TSV.

## Strategic Boundary

Layers owns context assembly and, on dedicated branches, autospawned research execution. The v2 stable-core contract is defined in [V2 Product Contract](V2_PRODUCT_CONTRACT.md), updated in the 2026-06-01 research-runtime pivot to permit a bounded overnight runtime as a first-class job, and broadened in the 2026-06-02 autospawn pivot to permit autospawned, continuously-chained runs as a v2.2 expansion of that job.

Stable Layers work should serve one of these jobs:

1. assemble context before a code change
2. preserve useful project memory after work completes
3. import/distill agent sessions into explicit memory
4. expose memory and impact context to other agents
5. verify that local context dependencies are healthy
6. run an autospawned, continuously-chained research-and-implementation cycle on a dedicated branch, producing a reviewable TSV and `[verified]` commits under a configurable cooldown and a soft wall-clock cap, never on the primary session or on protected branches

Work outside those jobs is non-essential and should be experimental, deprecated, or moved out of the core path. The autospawn runtime's blast radius is restricted to dedicated branches; it never holds the user's primary session, never auto-commits to `main`/`master`/release branches, and never edits memory outside `.layers/autoresearch/` and the dedicated branch's working tree.

## Stable Core

The v2.0 stable surface is defined by [V2 Product Contract](V2_PRODUCT_CONTRACT.md). Longer-term v2.x work should converge on:

- `layers query` / `layers preflight` — produce a context packet
- `layers packet validate` — validate persisted context packet artifacts
- `layers packet inspect` — summarize packet provenance, warnings, sections, and quality signals
- `layers packet render` — render persisted packets for human review, agent prompts, JSON normalization, or objective briefs
- `layers packet diff` — compare persisted packets as reviewable, body-safe artifacts
- `layers packet grade` — grade a packet against a workflow task spec for quality signals
- `layers remember` — write explicit project memory
- `layers memory` — list/search/show/retire/audit project memory
- `layers impact` — summarize blast radius for a file/symbol/task
- `layers import-session` — normalize agent sessions into a local event ledger
- `layers distill-session` — draft memory from a session
- `layers promote` — turn draft/council/session outputs into canonical memory
- `layers validate` / `layers doctor` — explain local readiness and degraded modes
- `layers refresh` — update GitNexus/MemoryPort derived context
- `layers mcp serve` — expose the stable core to other agents
- `layers research run` — autospawned, continuously-chained research-and-implementation cycle on a dedicated branch, with `--duration`, `--branch`, `--mode`, `--autospawn-cooldown`, and `--autospawn-trigger` flags; the sanctioned entrypoint for overnight and continuous work
- `layers research status` / `layers research stop` — cooperative introspection and cancellation for an in-flight research run, with a clean autospawn-loop exit that drains the in-flight iteration before shutdown

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
- subagent execution framework outside the research-runtime job
- messaging channels
- generic tool runtime
- infrastructure credential management
- autonomous monitor/fixer workflows outside the research-runtime job

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

Layers is the local-first context compiler **and** autospawned, continuously-chained research-and-implementation runtime for coding agents: it compiles project memory, Git/code intelligence, and prior sessions into auditable context packets before agents act, and it can run the same compile/implement/verify cycle back-to-back on a dedicated branch while the user is away.
