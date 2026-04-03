# Layers

A local-first Rust CLI that assembles grounded working context from structured memory and codebase analysis before you (or an agent) make changes to a repository.

Layers routes questions through two external local systems:

- **[MemoryPort](https://github.com/caelator/memoryport)** for durable memory and semantic retrieval, primarily via the local `uc` CLI and `~/.memoryport/uc.toml`
- **[GitNexus](https://github.com/caelator/gitnexus)** for codebase structure: call graphs, impact analysis, execution flows

Important integration note:

- **GitNexus** is a real MCP-capable tool surface and can also be reached through OpenClaw GitNexus skills.
- **MemoryPort** in this workflow is **not** assumed to be an MCP tool surface. The existing `codex-memoryport-bridge` is an OpenAI Responses-style memory injection proxy, not a generic MCP server. Layers therefore treats MemoryPort as a local CLI/service integration (`uc`, curated JSONL, optional proxy), not as a raw MCP tool provider.

The result is a **context packet** — a structured bundle of relevant memory and structural information — delivered as plain text or JSON.

## Why Layers Exists

AI coding agents and human developers both make better changes when they have the right context *before* they start editing. Layers exists to answer the question: **"What do I need to know before touching this code?"**

It is not a code editor, a hosted service, or a workflow platform. It is a small CLI that reads local data, shells out to local tools, and prints context.

## Status

Early software. Usable for local evaluation today.

| Category | State |
|----------|-------|
| `query`, `project`, `task`, `curated import`, `validate` | Stable enough for daily use |
| `refresh`, `remember`, `council run`, `council promote` | Useful but depends on external tool setup |
| Provider contracts, council ergonomics | Experimental |

If an external dependency is missing, Layers degrades gracefully — it uses whatever local data it can still read rather than failing outright.

## Quick Start

```bash
# Build from source (requires Rust 1.85+)
cargo build

# Install to your PATH
cargo install --path . --locked

# Run the health check
layers validate

# Ask a question
layers query "What constraints apply to the auth module?"

# Get JSON output
layers query "What did we decide about the data model?" --json
```

See [docs/walkthrough.md](docs/walkthrough.md) for a full getting-started guide.

## Commands

| Command | Purpose |
|---------|---------|
| `layers query <text>` | Route a question and return assembled context |
| `layers validate` | Health check across routing, providers, and records |
| `layers project create` | Create a structured project record |
| `layers project list` | List projects |
| `layers task create` | Create a task within a project |
| `layers task list` | List tasks (filterable by project, status) |
| `layers curated import <file>` | Import JSONL records into canonical memory |
| `layers refresh` | Re-index the repo via GitNexus |
| `layers remember <kind>` | Append workflow memory (plan, learning, trace) |
| `layers council run <task>` | Run a three-stage council workflow |
| `layers council promote <run_id>` | Promote a converged council run to canonical memory |

Full command reference: [docs/cli.md](docs/cli.md)

For repeatable benchmark checks, run:

```bash
layers validate --routing benchmarks/routing-answer-keys.jsonl --ci
```

## External Dependencies

Layers is intentionally small and shells out to local tools:

| Dependency | Required for | Install |
|------------|-------------|---------|
| **Rust 1.85+** | Building from source | [rustup.rs](https://rustup.rs) |
| **gitnexus** | Graph queries, impact analysis, `refresh` | `npm install -g gitnexus` |
| **uc** + `~/.memoryport/uc.toml` | Semantic memory retrieval and direct MemoryPort store/query operations | See MemoryPort docs |
| **codex-memoryport-bridge** (optional) | OpenAI/Codex Responses proxy with automatic memory injection | Local custom integration; not required for Layers CLI |
| **gemini**, **claude**, **codex** CLIs | Council workflow stages | Optional; configurable per-command |

**Without gitnexus:** `layers refresh` fails; graph-backed query results are empty. Everything else works.

**Without uc:** Semantic recall is unavailable. Layers still searches canonical structured records and local fallback files.

**Without codex-memoryport-bridge:** Nothing breaks in Layers itself. That bridge is for model-traffic augmentation, not for Layers retrieval.

**Without model CLIs:** `layers council run` fails. All other commands work normally.

## What Layers Is Not

- **Not a hosted service.** Everything runs locally, reads local files, and writes local files.
- **Not a stable provider API.** The interfaces between Layers and its external tools may change.
- **Not a workflow platform.** The council feature is a fixed three-stage pipeline, not a general orchestration engine.
- **Not a vector database.** Canonical data is structured JSONL. Embeddings are a retrieval optimization, not the source of truth.

## Data Model

Canonical project state lives in one file: `memoryport/curated-memory.jsonl`

Semantic retrieval is an optimization layered on top of canonical records via `uc`/MemoryPort; it is not the canonical store. Likewise, the local `codex-memoryport-bridge` can inject retrieved context into OpenAI/Codex Responses traffic, but it is not itself a canonical data store or MCP tool registry.

This is an append-friendly JSONL file containing typed records: projects, tasks, decisions, constraints, status snapshots, next steps, and postmortems. Each record has a standard envelope with an ID, entity type, timestamp, and payload.

Everything else under `memoryport/` (audit logs, council traces, council run directories) is generated local output — useful for debugging but not the source of truth.

Full data model documentation: [docs/data-model.md](docs/data-model.md)

## Documentation

| Document | Contents |
|----------|----------|
| [Walkthrough](docs/walkthrough.md) | Prerequisites through first workflow, step by step |
| [CLI Reference](docs/cli.md) | Every command, flag, and option |
| [FAQ](docs/faq.md) | Common questions and sharp edges |
| [Data Model](docs/data-model.md) | Canonical vs. generated files, record shapes |
| [Development](docs/development.md) | Build, test, validate, contribution workflow |
| [Release Readiness](docs/release-readiness.md) | What is ready, what is experimental, known gaps |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for scope, development loop, and validation expectations.

## License

MIT. See [LICENSE](LICENSE).
