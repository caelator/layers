# CLI Reference

Complete reference for the Layers command surface.

Layers is being narrowed around its North Star: a local-first context compiler for coding agents. Commands are labeled by stability so contributors know which surfaces are core and which are deprecated/experimental.

Status labels:

- **Stable core** — part of the intended v0.2 product surface.
- **Support** — useful developer/maintenance command, but not the product itself.
- **Beta** — useful if it feeds context or memory, but subject to change.
- **Deprecated / experimental** — not removed, but not core product direction. Do not expand without explicit justification against `docs/NORTH_STAR.md`.
- **Planned** — target for v0.2; may not exist yet.

---

## Stable Core Commands

## `layers query <text>`

Status: **Stable core**

Route a question through configured context providers and print an assembled context packet.

Arguments:

- `<text>` — the query/task string.

Options:

| Flag | Description |
|------|-------------|
| `--json` | Emit structured JSON instead of human-readable text |
| `--no-audit` | Skip appending an audit event |
| `--uc-min-results <n>` | Warn if semantic MemoryPort recall returns fewer than N results |

Examples:

```bash
layers query "What constraints apply before changing the auth module?"
layers query "What did we decide about provider routing?" --json
layers query "Show context for normalize_model_for_provider" --no-audit
```

Intended v0.2 behavior:

1. Build a `ContextPacket`.
2. Include relevant memory, graph context, failures, decisions, constraints, and suggested validation commands.
3. Include source citations and selection reasons.
4. Warn clearly when GitNexus, MemoryPort, or other providers are unavailable.

Current behavior:

1. Pattern-matches the query to determine a routing mode: `memory_only`, `graph_only`, `both`, or `neither`.
2. Retrieves results from applicable providers: curated records, semantic search via `uc`, and GitNexus graph when available.
3. Assembles and prints a context packet-like response.
4. Appends an audit event to `memoryport/layers-audit.jsonl` unless `--no-audit` is used.

## `layers remember <kind>`

Status: **Stable core**

Append explicit workflow memory to dedicated JSONL storage files.

Kinds:

- `plan`
- `learning`
- `trace`

Options:

| Flag | Required | Description |
|------|----------|-------------|
| `--task <name>` | Required for plan/trace unless summary supplied | Task identifier |
| `--summary <text>` | Required for learning or trace without task | Human-readable summary |
| `--file <path>` | Required for plan | Path to markdown plan file |
| `--task-type <type>` | No | Type classification |
| `--artifacts-dir <dir>` | No | Related artifacts directory |
| `--targets <symbols>` | No | Comma-separated GitNexus target symbols |

Examples:

```bash
layers remember learning \
  --summary "DeepSeek native API rejects non-native model IDs" \
  --targets normalize_model_for_provider

layers remember plan \
  --task "ContextPacket v1" \
  --file docs/ROADMAP_v0.2.md
```

v0.2 target:

Replace stringly `kind` handling with typed memory commands for decisions, constraints, failures, plans, test commands, status notes, architecture notes, handoffs, and open questions.

## `layers curated import <file>`

Status: **Stable core**

Import JSONL records from an external file into the canonical curated memory store.

Arguments:

- `<file>` — path to JSONL records in the standard envelope format.

Behavior:

Records are merged into `memoryport/curated-memory.jsonl`. See `docs/data-model.md` for the expected record format.

Example:

```bash
layers curated import ./exported-records.jsonl
```

## `layers validate`

Status: **Stable core**

Run a health check across routing, provider reachability, memory workflows, graph workflows, and record shape validation.

Options:

| Flag | Description |
|------|-------------|
| `--routing <file>` | Run answer-key routing benchmarks from a JSONL file |
| `--ci` | Exit non-zero if validation or routing benchmarks fail |

Examples:

```bash
layers validate
layers validate --routing benchmarks/routing-answer-keys.jsonl
layers validate --routing benchmarks/routing-answer-keys.jsonl --ci
```

Note:

`validate` can currently pass with degraded providers when typed-memory and fallback paths satisfy checks. v0.2 should make this more explicit through `layers doctor` or improved validate output.

## `layers refresh`

Status: **Stable core**

Refresh derived context sources.

Behavior:

- Runs `gitnexus analyze` on the workspace root.
- Preserves `.gitnexus` embeddings mode when possible.
- Flushes/checks MemoryPort through `uc` when available.
- Outputs JSON status on completion.

Requires:

- `gitnexus` on `PATH` for graph refresh.

Example:

```bash
layers refresh
```

## `layers config`

Status: **Stable core**

Inspect local configuration.

Subcommands:

- `show`
- `path`
- `validate`

## `layers packet validate <packet.json>`

Status: **Stable core**

Validate a persisted `ContextPacket` artifact without retrieving context, mutating the workspace, or running agents.

Arguments:

- `<packet.json>` — path to a persisted `ContextPacket` JSON artifact.

Options:

| Flag | Description |
|------|-------------|
| `--strict` | Treat degraded, low-confidence, or warning-bearing packets as invalid |
| `--json` | Append a structured validation report |

Example:

```bash
layers packet validate docs/examples/context-packet-v2-minimal.json --strict
```

## `layers packet inspect <packet.json>`

Status: **Stable core**

Summarize a persisted `ContextPacket` artifact for review. Inspection reports packet metadata, provenance, section/item counts, warnings, budget use, and quality signals; it does not echo full item bodies by default.

Options:

| Flag | Description |
|------|-------------|
| `--json` | Emit the inspection report as JSON |

Example:

```bash
layers packet inspect docs/examples/context-packet-v2-minimal.json --json
```

## `layers packet render <packet.json>`

Status: **Stable core**

Render a persisted `ContextPacket` artifact into an agent-neutral review or handoff format. This command is artifact-only: it validates and renders the packet that already exists, but does not retrieve more context, schedule work, execute tools, mutate files, or call an agent.

Options:

| Flag | Description |
|------|-------------|
| `--format <format>` | One of `markdown`, `agent-prompt`, `json`, or `objective-brief` |

Formats:

- `markdown` — human-readable packet rendering.
- `agent-prompt` — prompt-oriented packet rendering for a downstream coding agent.
- `json` — normalized pretty JSON for the same validated packet.
- `objective-brief` — concise work-unit brief with objective, constraints, citations, validation guidance, and handoff expectations.

Examples:

```bash
layers packet render docs/examples/context-packet-v2-minimal.json --format markdown
layers packet render docs/examples/context-packet-v2-minimal.json --format objective-brief
```

## `layers packet diff <old.json> <new.json>`

Status: **Stable core**

Compare two persisted `ContextPacket` artifacts as reviewable data. The diff is artifact-only and body-safe: it reports changed metadata, sections, items, warnings, budget, provenance, and retrieval fields without echoing full context item bodies in the human text output.

Options:

| Flag | Description |
|------|-------------|
| `--json` | Emit the semantic diff report as JSON |

Example:

```bash
layers packet diff before.packet.json after.packet.json --json
```

## `layers packet grade <packet.json>`

Status: **Stable core**

Grade a persisted `ContextPacket` artifact against a workflow task spec. This is artifact-only: it reads the existing packet and task spec from disk, evaluates quality signals such as target coverage, citation completeness, validation command presence, and warning state, then produces a structured quality report. It does not retrieve more context, compile a new packet, or call an agent.

Arguments:

- `<packet.json>` — path to a persisted `ContextPacket` JSON artifact.

Options:

| Flag | Required | Description |
|------|----------|-------------|
| `--task <task>` | Required | Path to a workflow task spec JSON artifact |
| `--json` | No | Emit the quality report as structured JSON |

Example:

```bash
layers packet grade packet.json --task benchmarks/workflows/tasks/code-bugfix-context-routing.json
layers packet grade packet.json --task task-spec.json --json
```

## `layers memory ...`

Status: **Stable core**

Inspect explicit project memory through the existing curated/remembered memory backend. This is a stable UX alias over the current curated-memory records; it does not introduce a separate memory store.

Subcommands:

```bash
layers memory list [--limit 20] [--include-legacy]
layers memory search <query> [--limit 10] [--include-legacy]
layers memory show <id> [--include-legacy]
```

## `layers impact <target>`

Status: **Stable core**

Produce Git-aware blast-radius context for a symbol, file, or task target. When GitNexus is available, Layers calls `gitnexus impact`; otherwise it degrades to a bounded local summary with explicit warnings and validation commands.

Options:

| Flag | Description |
|------|-------------|
| `--json` | Emit the impact report as JSON |
| `--include-tests` | Ask GitNexus to include tests and include test validation guidance |
| `--depth <n>` | Relationship depth for GitNexus impact, default `2` |

Expected output:

- source/status (`gitnexus`, `local`, or degraded)
- affected files when known
- validation commands
- degradation warnings when structural impact is unavailable

## Planned Stable Commands

These are the next commands Layers should add or stabilize after the current context-compiler slice.

## `layers import-session ...`

Status: **Planned stable**

Normalize prior agent sessions into an append-only local session ledger.

Initial importers:

- Hermes
- Claude Code
- Codex
- generic JSONL

## `layers distill-session <id>`

Status: **Planned stable**

Draft memory records from a normalized session. Drafts require explicit promotion before they become canonical memory.

## `layers mcp serve`

Status: **Stable core**

Serve the stable context compiler MCP surface over stdio. The default surface is intentionally narrow and excludes generic runtime/filesystem/process/subagent tools.

Stable MCP tool names:

- `context_compile`
- `impact_analyze`
- `memory_get`
- `memory_search`
- `preflight_context`
- `validate_context`

Example:

```bash
layers mcp serve
```

Use `layers query`, `layers preflight`, and `layers packet ...` for direct CLI workflows; use `layers mcp serve` when another agent/runtime needs the same stable context surface over MCP.

---

## Support Commands

## `layers gate`

Status: **Support**

Run the repo quality gate: format, compile, clippy, test, audit, and MCP ping.

Options:

| Flag | Description |
|------|-------------|
| `--skip-mcp` | Skip MCP connectivity check |
| `--audit-timeout <secs>` | Override `cargo audit` timeout |
| `--workspace <path>` | Workspace to gate |

## `layers feedback`

Status: **Support**

Record a route correction to improve future routing decisions.

This is useful for improving `layers query`, so it remains support/core-adjacent.

## `layers migrate`

Status: **Support**

Migrate legacy project records into canonical curated memory.

Options:

| Flag | Description |
|------|-------------|
| `--dry-run` | Preview without writing |

## `layers init`

Status: **Support**

Bootstrap a Layers workspace.

This command is useful only if it initializes the stable context/memory surfaces. It should not bootstrap deprecated runtime surfaces by default.

---

## Beta Commands

## `layers council run <task>`

Status: **Beta**

Execute a fixed three-stage council workflow:

1. Gemini generates proposals.
2. Claude critiques those proposals.
3. Codex synthesizes a converged recommendation.

This remains valuable when council outputs can be promoted into explicit project memory. It should not become a general orchestration framework.

## `layers council promote <run_id>`

Status: **Beta**

Promote a completed, converged council run into canonical curated memory.

This is aligned with the North Star because it converts deliberation into durable project memory.

## Other `layers council` subcommands

Status: **Beta**

- `resume`
- `resume-last`
- `status`
- `list`

These support the council memory-production workflow, but are not the core product.

---

## Deprecated / Experimental Commands

The commands below are not removed, but they are deprecated as core product direction. They duplicate mature agent frameworks or sit outside the context-compiler thesis.

## `layers chat`

Status: **Deprecated / experimental**

Reason:

General chat loops are already handled better by Hermes, Claude Code, Codex, OpenClaw, DeerFlow, Letta, and similar systems.

Allowed future work:

Only maintain enough to dogfood context packets or demonstrate integrations.

## `layers daemon`

Status: **Deprecated / experimental**

Reason:

A daemon-first runtime competes with mature agent gateways and makes Layers harder to understand.

Allowed future work:

Only expose stable context/memory/impact APIs if needed by MCP or local integrations.

## `layers monitor`

Status: **Deprecated / experimental**

Reason:

Autonomous repo monitoring/fixing is agent-runtime behavior. It should not drive Layers architecture.

Allowed future work:

Only monitor context dependency health if folded into `doctor`/`validate`.

## `layers technician`

Status: **Deprecated / experimental**

Reason:

Self-healing integration logic is useful but broad. It should be scoped to context dependency health, not general repair automation.

## `layers infrastructure`

Status: **Deprecated / experimental**

Reason:

Infrastructure credential management is outside the context compiler thesis.

## `layers telemetry`

Status: **Deprecated / experimental**

Reason:

Telemetry is useful for diagnostics, but should not become a standalone product axis.

## Integration Reality

Layers currently assumes:

- GitNexus is reachable via local CLI and/or MCP-backed systems.
- MemoryPort is reachable via `uc` and local canonical files.
- `codex-memoryport-bridge` is optional model-traffic augmentation, not a generic MCP provider.
- Model CLIs are optional and only required for council/experimental runtime features.

## Bootstrap Reality

Stable-core builds do not require the deprecated compatibility storage dependency:

```bash
cargo check --no-default-features --all-targets
cargo clippy --no-default-features --all-targets -- -D warnings
```

Default-feature builds still enable `substrate-storage` for monitor/technician/prove-it compatibility. That feature uses the optional git `substrate` dependency:

```toml
substrate = { git = "https://github.com/caelator/substrate.git", optional = true }
```

A fresh clone can run full default-feature development without a sibling `substrate` checkout:

```bash
cargo check --workspace --all-targets
```

The `proveit` binary is compatibility tooling and requires `substrate-storage`; no-default all-target checks intentionally skip it.
