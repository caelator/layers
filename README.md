# Layers

Layers is the local-first context compiler for coding agents.

It turns repository structure, Git history, code graph intelligence, project memory, prior agent sessions, decisions, constraints, failures, and plans into bounded, cited, reproducible context packets that agents can consume before they edit.

Layers is not trying to be another Hermes, OpenClaw, DeerFlow, Letta, mem0, Graphiti, or Cognee. Those systems are execution layers, agent platforms, or memory backends. Layers is the context spine they can consult.

## The Problem

AI coding agents are good at acting. They are still bad at reliably knowing what local context matters before they act.

They forget:

- previous failed attempts
- active project constraints
- architecture decisions
- fragile files and tests
- Git history and branch-local facts
- prior sessions from other agents
- why code is shaped the way it is

Layers answers:

> What does this agent need to know before touching this code?

## What Layers Produces

The core artifact is a context packet.

A context packet includes:

- relevant memories
- active decisions and constraints
- known failures and pitfalls
- relevant files, symbols, and execution flows
- GitNexus-backed impact analysis when available
- suggested validation commands
- source citations
- selection reasons
- warnings when context is degraded, stale, conflicting, or incomplete

The packet can be emitted as JSON, Markdown, or an agent-ready prompt.

## What Layers Is

Layers is:

- a local-first Rust CLI
- a deterministic context packet generator
- an explicit project memory ledger
- a Git/code-graph adapter around GitNexus
- a MemoryPort/uc integration surface
- an importer/distiller for prior agent sessions
- an MCP/CLI surface for Hermes, Claude Code, Codex, OpenClaw, DeerFlow, and other agents

## What Layers Is Not

Layers is not:

- a personal assistant
- a hosted service
- a chat-first product
- a general agent runtime
- a messaging gateway
- a generic model-provider platform
- a replacement for mature agent frameworks
- a replacement for generic memory systems

Non-essential runtime surfaces still exist in the repository, but they are deprecated as product direction unless they directly support context packet generation.

See [docs/NORTH_STAR.md](docs/NORTH_STAR.md) for the binding product direction and [docs/V2_PRODUCT_CONTRACT.md](docs/V2_PRODUCT_CONTRACT.md) for the v2 stable-core contract.

## Stable Core

These commands define the current and near-term product surface. The v2.0 stable-core boundary is stricter and is defined in [docs/V2_PRODUCT_CONTRACT.md](docs/V2_PRODUCT_CONTRACT.md).

These commands define the currently implemented stable/support surface:

| Command | Status | Purpose |
|---------|--------|---------|
| `layers query <text>` | Stable core | Assemble a context packet for a task |
| `layers remember <kind>` | Stable core | Add explicit project memory |
| `layers curated import <file>` | Stable core | Import canonical JSONL records |
| `layers validate` | Stable core | Check local readiness and degraded modes |
| `layers refresh` | Stable core | Refresh GitNexus/MemoryPort derived context |
| `layers config` | Stable core | Inspect local configuration |
| `layers gate` | Support | Run repo quality checks |

Near-term v0.2 additions:

| Command | Status | Purpose |
|---------|--------|---------|
| `layers memory ...` | Planned stable | List/search/show/retire/audit memory |
| `layers impact <target>` | Planned stable | Produce Git-aware blast-radius context |
| `layers import-session ...` | Planned stable | Normalize agent sessions into a local ledger |
| `layers distill-session <id>` | Planned stable | Draft memories from sessions |
| `layers mcp serve` | Planned stable | Expose context/memory/impact tools to agents |

## Deprecated as Core Direction

These commands/features are not removed, but they are no longer core product direction:

| Surface | Status | Reason |
|---------|--------|--------|
| `layers chat` | Deprecated / experimental | Duplicates mature agent chat runtimes |
| `layers daemon` | Deprecated / experimental | Should not be the primary product surface |
| web portal | Deprecated / experimental | Duplicates agent UIs; useful only for demos/inspection |
| generic provider runtime | Deprecated / experimental | Duplicates Hermes/OpenClaw/DeerFlow/Letta |
| generic tool runtime | Deprecated / experimental | Duplicates existing agent frameworks |
| subagent orchestration | Deprecated / experimental | Duplicates DeerFlow/Hermes/OpenClaw |
| messaging channels | Deprecated / experimental | Duplicates OpenClaw/Hermes gateways |
| infrastructure credential manager | Deprecated / experimental | Outside the context-compiler thesis |
| autonomous monitor/fixer | Deprecated / experimental | Useful only if scoped to context health |

## Quick Start

```bash
# Build from source. Requires Rust 1.85+.
cargo build

# Check local readiness.
cargo run -- validate

# Ask for context before work.
cargo run -- query "What constraints apply before changing the auth module?"

# Emit JSON for tooling.
cargo run -- query "What did we decide about model routing?" --json
```

## Bootstrap Reality

Layers' stable context-compiler core can be checked without deprecated compatibility dependencies:

```bash
cargo check --no-default-features --all-targets
```

Default-feature builds still include compatibility storage used by deprecated monitor/technician/prove-it artifacts. Those compatibility paths use the optional git `substrate` dependency:

```toml
substrate = { git = "https://github.com/caelator/substrate.git", optional = true }
```

A fresh clone no longer needs a hidden sibling checkout for stable-core or default-feature development. The dependency is intentionally isolated behind the `substrate-storage` default feature so the stable core does not require compatibility storage. The `proveit` binary is compatibility tooling and requires `substrate-storage`.

## External Dependencies

Layers is intentionally useful in degraded mode.

| Dependency | Required for | Degraded behavior when missing |
|------------|--------------|--------------------------------|
| Rust 1.85+ | Building | Required |
| GitNexus | Code graph, impact analysis, execution flows | Graph sections empty; packet warns |
| `uc` + `~/.memoryport/uc.toml` | MemoryPort semantic retrieval | Structured local memory still works |
| MemoryPort proxy/bridge | Optional model-traffic memory injection | Not required by Layers |
| Claude/Codex/Gemini CLIs | Council experiments | Stable context commands still work |

## Core Workflow

### 1. Before editing

```bash
layers query "Fix DeepSeek provider 404 in Hermes" --json
```

Agent uses the context packet to see:

- prior decisions
- known provider gotchas
- relevant files
- suggested tests
- risks

### 2. Preserve the learning

```bash
layers remember failure \
  --summary "DeepSeek native API rejects non-native model IDs like deepseek-v4-pro" \
  --targets normalize_model_for_provider
```

### 3. Refresh derived context

```bash
layers refresh
```

### 4. Hand context to another agent

Near-term v0.2 target:

```bash
layers query "Continue DeepSeek provider work" --agent-prompt
```

## Data Model

Canonical curated memory currently lives at:

```text
memoryport/curated-memory.jsonl
```

It is an explicit, reviewable, append-friendly JSONL memory spine. Generated operational files under `memoryport/` are useful for debugging but should not be treated as canonical memory unless promoted.

See [docs/data-model.md](docs/data-model.md).

## Roadmap

The v0.2 roadmap is a scope reset around the context-compiler thesis:

1. truthful bootstrap and degraded-mode diagnostics
2. ContextPacket v1
3. explicit memory inspection/audit commands
4. Git-aware `layers impact`
5. session import/distillation
6. MCP tools for other agents
7. context-quality benchmarks
8. integration docs for Hermes/Claude/Codex/OpenClaw

See [docs/ROADMAP_v0.2.md](docs/ROADMAP_v0.2.md).

## Documentation

| Document | Contents |
|----------|----------|
| [North Star](docs/NORTH_STAR.md) | Product direction and non-goals |
| [v0.2 Roadmap](docs/ROADMAP_v0.2.md) | Detailed plan to make Layers useful |
| [Production Readiness + Dogfood Plan](docs/PRODUCTION_READINESS_DOGFOOD_PLAN.md) | Release gates and dogfood workflow |
| [CLI Reference](docs/cli.md) | Command reference and stability labels |
| [Walkthrough](docs/walkthrough.md) | Getting started workflow |
| [Data Model](docs/data-model.md) | Canonical and generated memory files |
| [Development](docs/development.md) | Build/test/development loop |
| [Release Readiness](docs/release-readiness.md) | Current maturity notes |

## License

MIT. See [LICENSE](LICENSE).
