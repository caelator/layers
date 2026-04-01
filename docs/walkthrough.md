# Walkthrough

This guide takes you from a fresh clone to running your first query, creating project records, and understanding what Layers produces.

## Prerequisites

- **Rust 1.85 or newer.** Install via [rustup](https://rustup.rs).
- **A git repository** to work in. Layers resolves its workspace root from the nearest `.git` directory.

Optional (enables additional features):

- **gitnexus** on your `PATH` — enables graph-backed queries and impact analysis. Install with `npm install -g gitnexus`.
- **uc** and a config at `~/.memoryport/uc.toml` — enables semantic memory retrieval via Memoryport embeddings.
- **gemini**, **claude**, and/or **codex** CLIs — enables the council workflow.

You can use Layers without any of the optional tools. It will work with whatever local data is available and skip features that require missing tools.

## Build and Install

Clone the repo and build:

```bash
git clone https://github.com/caelator/layers.git
cd layers
cargo build
```

To install the `layers` binary to your Cargo bin directory:

```bash
cargo install --path . --locked
```

After this, `layers` is available as a command anywhere on your system.

## Verify the Installation

Run the built-in health check:

```bash
layers validate
```

This checks routing logic, provider reachability, memory retrieval, graph retrieval, and record shape validity. You will see a pass/fail summary. Some checks may report degraded status if optional tools are not installed — that is expected.

## Your First Query

```bash
layers query "What decisions have been made about the data model?"
```

Layers will:

1. **Route** your question — pattern matching determines whether to search memory, the code graph, both, or neither.
2. **Retrieve** relevant records from canonical curated memory (`memoryport/curated-memory.jsonl`), semantic search (if `uc` is available), and/or the GitNexus graph (if indexed).
3. **Assemble** a context packet combining the results.
4. **Print** the context to stdout.

To get structured JSON output instead:

```bash
layers query "What decisions have been made about the data model?" --json
```

By default, each query also appends an audit event to `memoryport/layers-audit.jsonl`. To suppress this:

```bash
layers query "some question" --no-audit
```

## Creating Projects and Tasks

Layers stores structured project records locally in `memoryport/curated-memory.jsonl`.

Create a project:

```bash
layers project create my-project "My Project Title" \
  --summary "A short description of this project" \
  --status active
```

List projects:

```bash
layers project list
layers project list --json
```

Create a task within a project:

```bash
layers task create my-project first-task "Implement the thing" \
  --summary "Details about what needs doing" \
  --status in_progress \
  --priority high \
  --acceptance "Tests pass, docs updated"
```

List tasks:

```bash
layers task list
layers task list --project my-project
layers task list --status in_progress --json
```

These records become part of the canonical memory that `layers query` searches.

## Importing Curated Records

If you have structured records in a JSONL file (perhaps exported from another tool or handwritten), import them:

```bash
layers curated import ./my-records.jsonl
```

Records are merged into `memoryport/curated-memory.jsonl`. See [data-model.md](data-model.md) for the expected record envelope format.

## Refreshing the Code Graph

If you have `gitnexus` installed, index (or re-index) your repository:

```bash
layers refresh
```

This runs `gitnexus analyze` on the current repo. If the repo already has embeddings configured (check `.gitnexus/meta.json`), Layers preserves that setting automatically.

After refreshing, graph-backed queries will return results about code structure, call graphs, and execution flows.

## Recording Workflow Memory

The `remember` command stores explicit workflow artifacts:

```bash
# Store a plan
layers remember plan --task "release-prep" --file ./plan.md

# Store a learning
layers remember learning --summary "uc config must specify the correct collection name"

# Store a trace
layers remember trace --task "debug-routing" --summary "Fixed routing for graph-only queries"
```

These records are written to dedicated JSONL files under `memoryport/` and are available to future queries.

## Running a Council Workflow

The council is a fixed three-stage pipeline that sends a task through three AI models in sequence:

1. **Gemini** generates options
2. **Claude** critiques those options
3. **Codex** produces a converged recommendation

```bash
layers council run "Design the caching strategy for query results" \
  --gemini-cmd gemini \
  --claude-cmd claude \
  --codex-cmd codex \
  --targets build_context,route_query
```

The `--targets` flag tells Layers which code symbols to include as structural context for the council. Artifacts are written to `memoryport/council-runs/<run_id>/`.

If the council run converges successfully, you can promote its output into canonical curated memory:

```bash
layers council promote <run_id> --project my-project
```

Use `--dry-run` to preview the record before writing it.

## Expected File Artifacts

After normal use, your `memoryport/` directory will contain:

| File | Type | Purpose |
|------|------|---------|
| `curated-memory.jsonl` | **Canonical** | Structured project records — the source of truth |
| `layers-audit.jsonl` | Generated | Query audit log (routing decisions, timing, result counts) |
| `council-plans.jsonl` | Generated | Stored plan records from `remember plan` |
| `council-learnings.jsonl` | Generated | Stored learnings from `remember learning` |
| `council-traces.jsonl` | Generated | Stored traces from `remember trace` |
| `council-runs/<run_id>/` | Generated | Per-run council artifacts (context, prompts, outputs, convergence) |

**Canonical files** should be versioned and reviewed. **Generated files** are local operational output — useful for debugging and replay, but not the source of truth.

The `.gitnexus/` directory at the repo root contains the GitNexus index. It is generated and should not be committed.

## Next Steps

- Read the [CLI Reference](cli.md) for every flag and option.
- Read the [FAQ](faq.md) for common questions and edge cases.
- Read [data-model.md](data-model.md) if you want to understand or hand-edit record structures.
- Read [development.md](development.md) if you want to contribute.
