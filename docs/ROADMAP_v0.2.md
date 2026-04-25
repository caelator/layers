# Layers v0.2 Roadmap — Context Compiler Reset

## Goal

Make Layers work and be useful as a focused local-first context compiler for coding agents.

v0.2 is not a general agent framework release. It is the release where Layers becomes reliably useful for:

1. assembling context before edits
2. preserving explicit project memory after work
3. importing/distilling prior agent sessions
4. exposing that context to other agents through CLI and MCP

## Product Thesis

Agents are already good at acting. They are still bad at knowing what local context matters before they act.

Layers should fill that gap.

## Scope Rules

### Invest

- ContextPacket schema and renderers
- explicit memory records
- GitNexus-backed impact context
- MemoryPort/uc semantic recall as an optional enhancement
- session import and distillation
- MCP tools that expose context/memory/impact
- degraded-mode diagnostics
- context-quality benchmarks

### Maintain but do not expand

- council workflow, as a producer of promotable memory
- refresh/validate/gate, as developer support
- technician only when scoped to context dependency health

### Deprecate as core direction

- chat portal
- daemon-first UX
- general model-provider runtime
- messaging channels
- generic tool runtime
- autonomous monitor/fixer workflows
- infrastructure credential management
- subagent framework

Deprecated does not mean deleted. It means these surfaces are not allowed to drive architecture or roadmap until they are explicitly re-justified against the North Star.

## Milestones

## M0 — Repo Health and Truthful Bootstrap

Objective: A fresh clone should have clear, truthful setup and degraded-mode behavior.

Tasks:

1. Resolve or document the `../substrate` path dependency.
   - Preferred: move it into the workspace or replace it with a git/crates.io dependency.
   - Acceptable short-term: document exact clone path and command.
   - Not acceptable: hidden sibling dependency.

2. Add `scripts/bootstrap.sh`.
   - Check Rust version.
   - Check substrate dependency.
   - Check `gitnexus`.
   - Check `uc` and `~/.memoryport/uc.toml`.
   - Check optional model CLIs.
   - Print stable/beta/experimental capability matrix.

3. Add or alias `layers doctor`.
   - Required core readiness.
   - Optional semantic memory readiness.
   - Optional graph readiness.
   - Optional council readiness.
   - Explicit degraded modes.

4. Ensure the stable core can build/test without experimental surfaces if possible.

Acceptance:

- Fresh clone setup is explicit.
- `cargo test` failure modes are documented or fixed.
- `layers validate`/`layers doctor` tells the truth.

## M1 — ContextPacket v1

Objective: Make the context packet the core artifact of Layers.

Tasks:

1. Add `ContextPacket` types in `crates/layers-core`.

Required fields:

- `id`
- `workspace_id`
- `query`
- `created_at`
- `git_ref`
- `budget`
- `sections`
- `warnings`
- `provenance`
- `selection_trace`

2. Add `ContextItem` and `ContextSource`.

Each item must include:

- title
- body/snippet
- source kind and URI
- confidence/score
- token estimate
- selected reason

3. Add renderers:

- JSON
- Markdown
- agent prompt

4. Update `layers query` to emit ContextPacket v1.

5. Add snapshot/serde roundtrip tests.

Acceptance:

- `layers query "..." --json` returns stable ContextPacket v1.
- `layers query "..." --agent-prompt` is directly pasteable into an agent.
- Every item has source and selection rationale.

## M2 — Explicit Project Memory v1

Objective: Make curated memory useful without requiring a separate memory platform.

Tasks:

1. Normalize record types:

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

2. Add memory commands:

- `layers memory list`
- `layers memory search <query>`
- `layers memory show <id>`
- `layers memory retire <id>`
- `layers memory audit`

3. Extend `layers remember` with typed subcommands where practical.

4. Add memory hygiene fields:

- status: active/superseded/retired
- confidence
- source
- created_by
- supersedes
- tags
- linked files/symbols

5. Add basic conflict detection.

Acceptance:

- Project memory can be inspected and cleaned from CLI.
- Conflicting active memories are visible.
- Memory is explicit, reviewable, and versionable.

## M3 — Git-Aware Impact Context

Objective: Own the coding-specific context niche.

Tasks:

1. Add `layers impact <target>`.

For symbol/file/task targets, return:

- direct callers/dependents
- affected execution flows
- likely tests/commands
- related files
- recent commits touching target
- linked decisions/failures/constraints
- risk level

2. Normalize GitNexus output into ContextPacket sections.

3. Add fallback mode when GitNexus is unavailable:

- file search
- Git history
- clear warning that graph context is degraded

Acceptance:

- `layers impact <symbol>` is useful before editing.
- Missing GitNexus does not make Layers useless.

## M4 — Session Ledger and Importers

Objective: Let agents share institutional memory without using the same agent platform.

Tasks:

1. Define normalized session event schema.

Events:

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

2. Add importers:

- Hermes sessions
- Claude Code sessions
- Codex sessions
- generic JSONL

3. Store normalized sessions under a local append-only ledger.

4. Add `layers distill-session <id>`.

Output draft memory records; do not auto-promote.

5. Add promotion flow from drafts into canonical memory.

Acceptance:

- A prior Hermes/Claude/Codex session can become searchable project context.
- Useful learnings are promoted explicitly.

## M5 — MCP v1

Objective: Make Layers callable by other agents.

Tools:

1. `layers_context_packet`
2. `layers_remember`
3. `layers_impact`
4. `layers_memory_search`
5. `layers_promote`
6. `layers_doctor`

Acceptance:

- Hermes can call Layers for context before coding.
- The MCP surface mirrors stable CLI behavior.
- MCP does not expose deprecated agent-runtime surfaces by default.

## M6 — Context Quality Evaluation

Objective: Make context quality measurable.

Tasks:

1. Add benchmark fixtures under `benchmarks/context-packets/`.

Each case includes:

- query
- expected memory IDs
- expected graph targets
- forbidden irrelevant items

2. Add `layers eval context`.

3. Track:

- expected item recall
- irrelevant item rate
- missing critical constraint rate
- token budget utilization

Acceptance:

- Context-packet quality can improve without vibes-only evaluation.

## M7 — Documentation and Integrations

Objective: Make Layers easy to use with existing agents.

Docs:

- `docs/integrations/hermes.md`
- `docs/integrations/claude-code.md`
- `docs/integrations/codex.md`
- `docs/integrations/openclaw.md`
- `docs/integrations/mcp.md`

Examples:

- pre-edit context packet
- post-session distillation
- impact analysis before changing a symbol
- handoff to another agent

Acceptance:

- A user can make Hermes/Claude/Codex call Layers in under 10 minutes.

## 12-Week Execution Plan

### Week 1

- North Star doc
- README reset
- command stability labels
- substrate dependency plan

### Week 2

- ContextPacket v1 types
- JSON renderer
- serde/snapshot tests

### Week 3

- Markdown and agent-prompt renderers
- update `layers query`

### Week 4

- memory list/search/show/retire
- memory audit basics

### Week 5

- `layers impact` with GitNexus adapter
- fallback impact mode

### Week 6

- session event schema
- Hermes session importer

### Week 7

- Claude/Codex/generic importers
- distill-session drafts

### Week 8

- promotion flow
- handoff packet command

### Week 9

- MCP v1 tools
- Hermes integration smoke test

### Week 10

- context quality benchmark
- initial benchmark cases from real repos

### Week 11

- docs/integrations
- dogfood on `layers`, `hermes-agent`, and `triumvirate`

### Week 12

- v0.2 tag
- archive/de-emphasize deprecated surfaces
- publish examples and release notes

## First Implementation Batch

1. Finish docs reset.
2. Fix or document substrate dependency.
3. Add command labels.
4. Add ContextPacket v1.
5. Convert `layers query` to ContextPacket output.

## Definition of Done for v0.2

- Fresh clone instructions are truthful.
- Stable commands are clearly separated from deprecated/experimental commands.
- `layers query` produces cited ContextPacket v1 output.
- `layers remember` and memory inspection are useful for real project memory.
- `layers impact` provides Git-aware coding context.
- At least one external agent can call Layers through MCP.
- Deprecated runtime surfaces do not define the product or README.
