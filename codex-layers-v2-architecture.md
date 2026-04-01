# Layers v2 Architecture

## Purpose

Layers v2 should be a durable local orchestrator for context retrieval, not a general-purpose agent platform. Over a 5-year horizon, the system should keep a very small stable core:

- stable artifact types
- stable provider contracts
- stable workflow records
- stable auditability and refusal behavior

Everything else should remain replaceable.

The current Rust code already points in the right direction: routing, synthesis, audit logging, fallback memory search, and command handling are local and explicit. The main architectural problem is that the provider boundary is still implicit CLI parsing rather than a typed contract. v2 should fix that without introducing plugin theater.

## Architectural Principles

1. Local-first is non-negotiable. Layers should work against local repos, local artifacts, and local memory stores by default.
2. Rust-first applies to orchestration, contracts, and storage. External tools are acceptable providers, but the core contract should live in Rust.
3. Refusal is a feature. Layers should continue to prefer `neither` over speculative retrieval.
4. Stable contracts should be minimal. Only artifact schemas and provider request/response types should be considered long-lived.
5. Providers are replaceable, not sovereign. Memoryport and GitNexus are first-class, but Layers owns routing, workflow composition, synthesis, and audit.
6. Writeback stays explicit. Durable memory creation should remain a deliberate workflow step, not an automatic side effect of every query.
7. Provenance must survive. Every artifact and every provider result should preserve source, timestamp, and retrieval method.

## What Must Be Stable Now

These are the contracts worth freezing early because they let the rest of the system evolve safely.

### 1. Artifact envelope

Every persisted or exchanged Layers artifact should share a common envelope:

```json
{
  "artifact_type": "workflow.plan",
  "artifact_version": "1",
  "artifact_id": "uuid-or-stable-hash",
  "created_at": "2026-03-31T12:00:00Z",
  "created_by": "layers",
  "workspace": {
    "root": "/abs/repo",
    "repo_id": "optional-stable-id"
  },
  "provenance": {
    "provider": "memoryport",
    "provider_record_id": "optional",
    "source_path": "optional",
    "source_kind": "semantic_retrieval"
  },
  "payload": {}
}
```

This should be stable even if storage layout changes.

### 2. Provider traits

Layers should standardize provider interfaces now, even if the initial implementation still shells out to `uc` and `gitnexus`.

```rust
pub trait MemoryProvider {
    fn retrieve(&self, request: MemoryRetrieveRequest) -> anyhow::Result<MemoryRetrieveResponse>;
    fn append_record(&self, record: CuratedMemoryRecord) -> anyhow::Result<AppendOutcome>;
    fn health(&self) -> anyhow::Result<ProviderHealth>;
}

pub trait GraphProvider {
    fn query(&self, request: GraphQueryRequest) -> anyhow::Result<GraphQueryResponse>;
    fn refresh_index(&self, request: RefreshIndexRequest) -> anyhow::Result<RefreshIndexOutcome>;
    fn health(&self) -> anyhow::Result<ProviderHealth>;
}
```

The traits should be stable. The concrete transport should not.

### 3. Workflow record families

Freeze 3 workflow artifact families now:

- `workflow.plan`
- `workflow.handoff`
- `workflow.postmortem`

These cover most durable cross-session value without inventing a large ontology.

### 4. Audit event shape

The audit log shape should stay stable enough to compare versions and debug routing behavior:

- query
- route
- confidence
- scores
- rationale
- provider calls attempted
- provider issues
- result counts
- duration

## What Can Remain Internal Or Provisional

- routing heuristics and scoring tables
- synthesis wording and formatting
- ranking functions for memory hits
- graph fact normalization rules
- file layout under `memoryport/`
- exact CLI surface beyond a small core
- whether providers are backed by subprocesses, libraries, or local daemons

These should be allowed to change freely as long as the stable artifact and provider contracts remain intact.

## What Should Not Be Built Yet

- a dynamic plugin marketplace
- runtime-loaded providers from arbitrary third parties
- a generic DAG workflow engine
- distributed syncing as a first-class requirement
- autonomous background writeback
- over-general knowledge graphs inside Layers itself
- a huge typed ontology for every possible memory/event kind

Layers does not need to become its own platform. It needs to become a durable composition layer.

## Stable Artifact Types

The artifact model should stay intentionally small.

### Core artifact families

#### 1. `retrieval.context`

Ephemeral assembly artifact returned for one query or task.

Purpose:

- capture route decision
- bundle provider evidence
- expose uncertainty
- feed downstream agent/tool execution

Persistence:

- optional
- short-lived by default

#### 2. `workflow.plan`

Durable planning artifact for implementation, architecture, migration, or investigation work.

Stable fields:

- `title`
- `task`
- `objective`
- `constraints`
- `decisions`
- `steps`
- `acceptance_criteria`
- `related_artifacts`

#### 3. `workflow.handoff`

Durable transfer artifact between sessions, agents, or humans.

Stable fields:

- `title`
- `status`
- `current_state`
- `next_actions`
- `risks`
- `important_files`
- `related_artifacts`

#### 4. `workflow.postmortem`

Durable learning artifact after a bug, failure, migration, or review cycle.

Stable fields:

- `incident_or_task`
- `expected`
- `observed`
- `root_causes`
- `fixes`
- `follow_ups`
- `lessons`

#### 5. `memory.record`

Curated durable memory unit stored through a memory provider.

This is not the same thing as raw provider output. It is the normalized Layers memory object.

Stable fields:

- `record_type`
- `summary`
- `detail`
- `tags`
- `importance`
- `scope`
- `sources`
- `related_artifacts`

### Non-goal for v2 artifact design

Do not freeze the current raw JSONL shapes in `council-plans.jsonl`, `council-traces.jsonl`, and `council-learnings.jsonl` as the long-term public schema. They are a migration source, not the target contract.

## Provider Interfaces

The core move for v2 is to replace “call external CLI and parse ad hoc text inside command handlers” with typed provider adapters.

### Memory provider contract

#### Requests

```rust
pub struct MemoryRetrieveRequest {
    pub query: String,
    pub limit: usize,
    pub intent: MemoryIntent,
    pub workspace_root: std::path::PathBuf,
}
```

`MemoryIntent` should start small:

- `DecisionRecall`
- `TaskHistory`
- `PatternRecall`

#### Responses

```rust
pub struct MemoryRetrieveResponse {
    pub hits: Vec<MemoryRecordRef>,
    pub issue: Option<String>,
    pub backend: MemoryBackendKind,
}
```

`MemoryRecordRef` should normalize:

- record id
- record type
- summary
- score
- timestamp
- provenance

### Graph provider contract

#### Requests

```rust
pub struct GraphQueryRequest {
    pub query: String,
    pub limit: usize,
    pub intent: GraphIntent,
    pub workspace_root: std::path::PathBuf,
}
```

`GraphIntent` should start with:

- `StructureLookup`
- `ImpactLookup`
- `ExecutionTrace`

#### Responses

```rust
pub struct GraphQueryResponse {
    pub facts: Vec<GraphFact>,
    pub issue: Option<String>,
    pub backend: GraphBackendKind,
}
```

`GraphFact` should normalize:

- fact type
- subject
- path
- optional process
- optional step index
- provenance

### Provider implementation strategy

Initial implementations should be:

- `MemoryportCliProvider`
- `GitNexusCliProvider`

Later, Layers may add:

- `MemoryportLibraryProvider`
- `GitNexusLibraryProvider`
- `FilesystemMemoryProvider`
- `LspGraphProvider`

The core should not care.

## Curated Memory Records

Layers should distinguish between raw retrieved material and curated memory.

### Why this matters

The current code retrieves from two places:

- semantic retrieval through `uc`
- fallback JSONL records under `memoryport/`

That is useful, but it conflates retrieval source with durable memory model. v2 should introduce a single curated record schema and allow providers to map into or store that schema.

### Curated record schema

```json
{
  "artifact_type": "memory.record",
  "artifact_version": "1",
  "payload": {
    "record_type": "decision",
    "summary": "Layers should keep provider writeback explicit.",
    "detail": "Automatic writeback creates low-signal noise and weakens auditability.",
    "tags": ["layers", "memory", "writeback"],
    "importance": "high",
    "scope": {
      "workspace": "layers",
      "task_type": "architecture"
    },
    "sources": [
      {
        "kind": "workflow.plan",
        "artifact_id": "plan-123"
      }
    ],
    "related_artifacts": ["plan-123", "postmortem-004"]
  }
}
```

### Initial record types

- `decision`
- `constraint`
- `status`
- `lesson`
- `open_question`

That is enough for several years. Avoid more granularity until real usage forces it.

### Curation policy

Only explicit, high-signal material should become curated memory:

- accepted architecture decisions
- important constraints
- stable implementation lessons
- meaningful postmortem findings

Do not curate:

- every query
- transient chat residue
- low-signal traces
- redundant summaries that can be recomputed

## Recommended Workflows

Layers v2 should officially support three workflows now.

### 1. Planning workflow

Purpose:

- retrieve prior decisions
- retrieve relevant repo structure
- assemble implementation-conscious context
- produce a durable `workflow.plan`
- optionally derive curated `memory.record` entries

Typical flow:

1. Route query.
2. Call memory provider with `DecisionRecall` or `TaskHistory`.
3. Call graph provider with `StructureLookup` or `ImpactLookup` when needed.
4. Build `retrieval.context`.
5. Write `workflow.plan` if the result crosses the “durable planning output” threshold.
6. Curate selected decisions and constraints into `memory.record`.

### 2. Handoff workflow

Purpose:

- capture current state for another session or agent
- preserve active risks and next actions
- avoid re-reading the whole history next time

Typical flow:

1. Gather recent plan, current repo state, and open issues.
2. Build `workflow.handoff`.
3. Optionally store 1-3 `memory.record` entries for durable lessons or constraints.

### 3. Postmortem workflow

Purpose:

- convert execution failures or regressions into durable learning
- preserve root cause and fix rationale

Typical flow:

1. Gather incident context, changed files, and relevant historical memory.
2. Build `workflow.postmortem`.
3. Promote the highest-signal lessons into curated `memory.record`.

## How Current Rust Modules Should Evolve

The current modules are close to a useful v2 internal boundary. The change is mainly about introducing typed core contracts and moving provider-specific parsing to adapters.

### Recommended target layout

```text
src/
  main.rs
  cli/
    mod.rs
  core/
    artifacts.rs
    audit.rs
    routing.rs
    workflows.rs
    synthesis.rs
    providers.rs
    memory.rs
    graph.rs
  providers/
    memoryport_cli.rs
    gitnexus_cli.rs
  storage/
    artifact_store.rs
    memory_store.rs
  commands/
    query.rs
    refresh.rs
    remember.rs
    validate.rs
```

### Mapping from current modules

#### `src/types.rs`

Split into:

- `core::artifacts`
- `core::providers`

Add typed structs for:

- artifact envelope
- workflow payloads
- provider requests/responses
- audit events

`RouteDecision` can remain internal for now, but its serialized shape should be formalized in `core::routing` if exposed in artifacts.

#### `src/routing.rs`

Keep as internal policy logic. This should stay replaceable. The route names can remain stable:

- `memory_only`
- `graph_only`
- `both`
- `neither`

But the heuristic tables should not be considered public API.

#### `src/memory.rs`

Split into:

- `core::memory` for normalized memory models and ranking policy
- `providers::memoryport_cli` for `uc` invocation and output parsing
- `storage::memory_store` for local curated-memory persistence

This is the most important refactor.

#### `src/graph.rs`

Split into:

- `core::graph` for normalized graph fact types
- `providers::gitnexus_cli` for status/query/refresh parsing

Normalize GitNexus output into typed `GraphFact` values before synthesis.

#### `src/synthesis.rs`

Keep internal. It should consume normalized artifacts and provider responses, not raw CLI strings.

This module should own:

- `retrieval.context` assembly
- user-facing evidence brief generation
- uncertainty reporting

#### `src/commands.rs`

Shrink into workflow entrypoints only. It should orchestrate:

- route
- provider calls
- workflow execution
- persistence

It should not contain provider parsing or storage policy.

#### `src/config.rs`

Keep, but separate:

- environment/config resolution
- workspace identity
- provider configuration

Do not let config become a dumping ground for unrelated helpers.

#### `src/util.rs`

Keep generic helpers only. Do not store business logic here.

## Migration Plan

### Phase 1: Freeze contracts without breaking behavior

- Introduce typed artifact structs.
- Introduce typed provider request/response structs.
- Wrap current `uc` and `gitnexus` code behind provider adapters.
- Preserve current CLI behavior and audit output.

### Phase 2: Normalize durable storage

- Add a local artifact store for `workflow.plan`, `workflow.handoff`, and `workflow.postmortem`.
- Add curated `memory.record` persistence.
- Keep compatibility readers for existing `memoryport/*.jsonl`.

### Phase 3: Convert commands into workflows

- Reframe `query`, `remember`, `validate`, and `refresh` around workflow execution.
- Make `remember` write normalized artifacts rather than ad hoc JSON by kind.

### Phase 4: Tighten validation

- Validate provider health separately from contract behavior.
- Add fixture-backed tests for workflow artifacts and provider adapters.
- Make replacement readiness deterministic and environment-independent.

### Phase 5: Revisit provider transports

- Only after the typed core is stable, evaluate whether direct library integrations are worth the cost.
- If the CLI adapters are stable and fast enough, keep them.

## Validation And Compatibility Strategy

The most important long-term guarantee is not “same implementation” but “same contract”.

Layers v2 should test:

- route decisions
- provider adapter normalization
- artifact serialization/deserialization
- context assembly
- curated memory promotion rules
- audit event generation

Compatibility should be measured at:

- artifact shape
- provider response normalization
- workflow outcome

Not at:

- exact wording
- exact ranking scores
- exact internal module names

## Risks And Mitigations

### Risk: over-abstracting providers too early

Mitigation:

- only define two provider traits now
- only support two first-class implementations now

### Risk: freezing the wrong schemas

Mitigation:

- freeze envelope and workflow payload shape
- keep ranking/routing/synthesis provisional

### Risk: duplicate storage between Memoryport and local artifact store

Mitigation:

- treat Layers artifacts as canonical application data
- treat provider-specific raw outputs as integration details

### Risk: curated memory becomes noisy

Mitigation:

- require explicit promotion
- keep record types few
- prefer omission over clutter

### Risk: command handlers grow back into a god module

Mitigation:

- push provider parsing into adapters
- push persistence into stores
- keep commands workflow-oriented only

## Anti-Goals

- Layers should not become its own vector database.
- Layers should not reimplement GitNexus internally.
- Layers should not absorb Memoryport’s retrieval engine.
- Layers should not depend on a network service for core usage.
- Layers should not create autonomous memory continuously in the background.
- Layers should not lock itself into today’s CLI output formats as its public contract.

## Recommended Decision

The practical v2 decision is:

- freeze a small artifact model now
- freeze typed provider interfaces now
- support exactly three first-class workflows now
- add curated memory as a normalized Layers record, not as raw provider residue
- keep Memoryport and GitNexus as first-class providers behind adapters
- defer any broader platform or plugin ambitions

That gives Layers a stable core that can survive transport changes, storage changes, and future providers without destabilizing the system.
