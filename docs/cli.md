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

## Planned Stable Commands

These are the next commands Layers should add or stabilize for v0.2.

## `layers memory ...`

Status: **Planned stable**

Inspect and maintain explicit project memory.

Planned subcommands:

```bash
layers memory list
layers memory search <query>
layers memory show <id>
layers memory retire <id>
layers memory audit
```

## `layers impact <target>`

Status: **Planned stable**

Produce Git-aware blast-radius context for a symbol, file, or task.

Expected output:

- direct callers/dependents
- affected execution flows
- likely tests/commands
- related files
- recent commits touching target
- linked decisions/failures/constraints
- risk level

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

Status: **Planned stable**

Expose the stable context/memory/impact surface to other agents.

Planned MCP tools:

- `layers_context_packet`
- `layers_remember`
- `layers_impact`
- `layers_memory_search`
- `layers_promote`
- `layers_doctor`

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
