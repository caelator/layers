# FAQ

## General

### What is Layers for?

Layers assembles relevant context — past decisions, project constraints, code structure, impact analysis — before you or an AI agent starts editing code. It is a "look before you leap" tool.

### Who is the intended user?

Developers and AI coding agents working in repositories that benefit from structured project memory and codebase awareness. Layers is particularly useful when multiple agents or contributors work on the same codebase over time and need shared institutional knowledge.

### Is Layers a hosted service?

No. Everything runs locally. Layers reads local files, shells out to local tools, and writes local files. There is no server, no account, and no network dependency (unless your external tools have their own).

### What language is it written in?

Rust (edition 2024, minimum supported version 1.85). The only runtime dependencies are the Rust standard library and whichever external CLI tools you choose to configure.

---

## Installation and Setup

### Do I need all the external tools?

No. Only the Rust toolchain is required to build and run Layers.

| Tool | What you lose without it |
|------|-------------------------|
| gitnexus | No graph queries, no impact analysis, no `refresh` |
| uc + config | No semantic memory retrieval (structured record search still works) |
| gemini/claude/codex CLIs | No council workflow (all other commands work) |

Layers detects what is available and degrades gracefully.

### How do I know what is working?

Run `layers validate`. It checks routing, provider reachability, memory retrieval, graph retrieval, and record shapes. The output tells you what passed, what degraded, and what failed.

### Where does Layers store its data?

All data lives under `memoryport/` in your workspace root (typically the git repo root). The workspace root is resolved from `LAYERS_WORKSPACE_ROOT` env var, or the nearest `.git` directory, or the current working directory.

---

## Queries and Context

### How does query routing work?

Layers pattern-matches your query text to decide where to search:

- **Memory-only:** questions about decisions, constraints, status, learnings, postmortems
- **Graph-only:** questions about code structure, dependencies, modules, impact, architecture
- **Both:** questions that span project knowledge and code structure
- **Neither:** trivial or off-topic queries (local syntax questions, typos, etc.)

This is keyword/pattern-based routing, not an LLM call. It is fast but imperfect — some queries may route suboptimally.

### What is a "context packet"?

The structured output of a query. It combines:

- An architecture summary (decisions, constraints, structural notes, status)
- Detailed context (individual memory and graph hits with relevance scores)
- A memory brief (synthesized view of decisions, constraints, status, next steps, postmortems)

In `--json` mode, this is machine-readable. In plain text mode, it is formatted for human reading.

### What is the audit log?

Every query appends an event to `memoryport/layers-audit.jsonl` recording the query text, routing decision, result counts, and duration. This is generated operational data, not canonical memory. Suppress it with `--no-audit`.

---

## Data and Records

### What is `curated-memory.jsonl`?

The canonical source of truth for project state. It is an append-friendly JSONL file containing typed records: projects, tasks, decisions, constraints, status snapshots, next steps, and postmortems. Each record has a standard envelope with an ID, entity type, timestamp, and payload.

### Can I edit `curated-memory.jsonl` by hand?

Yes. It is a plain JSONL file — one JSON object per line. You can add, edit, or remove records with any text editor. Just maintain the envelope structure documented in [data-model.md](data-model.md).

### What is the difference between canonical and generated files?

**Canonical** (`curated-memory.jsonl`): the source of truth. Version it, review it, curate it.

**Generated** (audit logs, council traces, council run directories, `.gitnexus/`): operational output from running Layers. Useful for debugging and replay, but reproducible and not the source of truth. Generally should not be committed.

### What is `project-records.jsonl`?

A legacy compatibility path. If this file exists from an older version of Layers, it is read as a fallback input. New records are always written to `curated-memory.jsonl`.

---

## Council Workflow

### What is the council?

A fixed three-stage pipeline that sends a task through three AI models:

1. **Gemini** generates options or proposals
2. **Claude** critiques those proposals
3. **Codex** synthesizes a converged recommendation

Each stage receives the context packet plus the output of previous stages.

### Do I need all three model CLIs?

The council expects all three stages. You can point multiple stages at the same CLI if needed (e.g., `--gemini-cmd claude --claude-cmd claude --codex-cmd claude`), but the workflow is designed around model diversity.

### What does "promote" mean?

After a council run completes and converges, `layers council promote` takes the converged output and writes it as a canonical record in `curated-memory.jsonl`. This is how council recommendations become durable project memory.

Promotion fails if the run did not complete, did not converge, targets a nonexistent project, or was already promoted. Use `--dry-run` to preview.

### Where are council artifacts stored?

Under `memoryport/council-runs/<run_id>/`. Each run directory contains:

- `context.txt` / `context.json` — the input context packet
- Per-stage prompt and output files
- `run.json` — run metadata
- `convergence.json` — convergence assessment

---

## Troubleshooting

### `layers validate` passes but queries return empty results

Check which providers are actually reachable. `validate` can pass with just structured record search — if your curated memory file is empty and optional providers are unavailable, queries will naturally return little.

Add some records with `layers project create` or `layers curated import`, or install the optional tools.

### `layers refresh` fails

This requires `gitnexus` on your PATH. Check with `which gitnexus`. If it is not installed, graph-backed features will be unavailable but everything else works.

### Council runs fail immediately

Check that the model CLIs are installed and working independently. Try running your configured command (e.g., `gemini`, `claude`, `codex`) directly in your terminal to verify they work.

### Queries about code structure return nothing

This means GitNexus is either not installed or the index is stale/missing. Run `layers refresh` (requires `gitnexus`) to build or update the index.

### Semantic memory search returns nothing

This requires `uc` and a valid config at `~/.memoryport/uc.toml`. Without it, Layers falls back to structured record search only.
