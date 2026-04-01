# Layers v2 Architecture Summary

Layers v2 should be a small Rust core that composes local providers, not a plugin platform. The durable boundary for the next 5 years should be: stable artifact envelopes, stable workflow artifact types, stable provider request/response contracts, and stable audit events. Routing heuristics, ranking logic, synthesis wording, and transport details should remain replaceable.

## Stable Now

- Artifact envelope with version, workspace identity, provenance, and typed payload.
- Three durable workflow artifacts: `workflow.plan`, `workflow.handoff`, `workflow.postmortem`.
- One normalized curated memory artifact: `memory.record`.
- Two typed provider traits: `MemoryProvider` and `GraphProvider`.
- Stable audit fields for route, confidence, rationale, provider issues, counts, and duration.

## Internal Or Provisional

- Routing scores and regex rules.
- Memory ranking and graph fact distillation.
- Exact CLI surface.
- Whether providers use subprocesses, libraries, or another local transport.
- Current JSONL layouts under `memoryport/`.

## Do Not Build Yet

- Dynamic plugin loading.
- Generic workflow engines.
- Autonomous writeback.
- Giant ontologies.
- Internal replacement of Memoryport or GitNexus.

## First-Class Providers

Memoryport and GitNexus should stay first-class, but behind typed adapters:

- `MemoryportCliProvider` wraps `uc`.
- `GitNexusCliProvider` wraps `gitnexus`.

Future providers can fit by implementing the same traits without changing the core workflow layer.

## Three Recommended Workflows

1. Planning
   Produces `retrieval.context`, then a durable `workflow.plan`, and optionally promotes key decisions into `memory.record`.
2. Handoff
   Produces `workflow.handoff` with status, next actions, risks, and important files.
3. Postmortem
   Produces `workflow.postmortem` and promotes durable lessons into curated memory.

## How The Current Rust Code Should Evolve

- Keep `routing` and `synthesis` as internal policy modules.
- Split `memory.rs` into normalized memory types, a Memoryport adapter, and local memory storage.
- Split `graph.rs` into normalized graph facts and a GitNexus adapter.
- Shrink `commands.rs` into workflow orchestration only.
- Replace ad hoc JSONL-by-kind writes with typed artifact persistence.

## Practical Recommendation

Build v2 around a minimal stable contract:

- typed artifacts
- typed provider adapters
- explicit workflows
- explicit curated memory promotion

That is enough to make Layers durable and extensible without turning it into architecture theater.
