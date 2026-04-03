# CLI Reference

Complete reference for every Layers command, subcommand, and option.

---

## `layers query <text>`

Route a question through configured providers and print the assembled context packet.

**Arguments:**

- `<text>` — the query string (required)

**Options:**

| Flag | Description |
|------|-------------|
| `--json` | Emit structured JSON instead of plain text |
| `--no-audit` | Skip appending an audit event to the audit log |

**Examples:**

```bash
layers query "What constraints apply to the auth module?"
layers query "What did we decide about caching?" --json
layers query "Show me the call graph for handle_query" --no-audit
```

**Behavior:**

1. Pattern-matches the query to determine a routing mode: `memory_only`, `graph_only`, `both`, or `neither`.
2. Retrieves results from applicable providers (curated records, semantic search via `uc`, GitNexus graph).
3. Assembles and prints a context packet.
4. Appends an audit event to `memoryport/layers-audit.jsonl` (unless `--no-audit`).

**Integration note:** GitNexus is expected to be reachable as a local CLI/MCP-backed system. MemoryPort is expected to be reachable through `uc` and local canonical files. The existing `codex-memoryport-bridge` is a model proxy, not a generic MCP tool surface.

---

## `layers validate`

Run a health check across routing, provider reachability, memory workflows, graph workflows, and record shape validation.

**Options:**

| Flag | Description |
|------|-------------|
| `--routing <file>` | Run answer-key routing benchmarks from a JSONL file |
| `--ci` | Exit non-zero if validation or routing benchmarks fail |

**Output:** Pass/fail summary for each check, plus an overall status.

**Note:** `validate` can report overall success when semantic embedding access is unavailable, as long as typed-memory and fallback paths satisfy the current checks. This is intentional — it validates what you have, not what you could have.

```bash
layers validate
layers validate --routing benchmarks/routing-answer-keys.jsonl
layers validate --routing benchmarks/routing-answer-keys.jsonl --ci
```

---

## `layers project create <slug> <title>`

Create a new project record in canonical curated memory.

**Arguments:**

- `<slug>` — short identifier for the project (e.g., `my-project`)
- `<title>` — human-readable project title

**Options:**

| Flag | Description |
|------|-------------|
| `--summary <text>` | Project description |
| `--status <status>` | Project status (e.g., `active`, `planned`, `completed`) |

**Example:**

```bash
layers project create layers "Layers CLI" --summary "Local context assembly tool" --status active
```

---

## `layers project list`

List all project records.

**Options:**

| Flag | Description |
|------|-------------|
| `--json` | Emit JSON output |

---

## `layers task create <project> <slug> <title>`

Create a task record within an existing project.

**Arguments:**

- `<project>` — the project slug this task belongs to
- `<slug>` — short identifier for the task
- `<title>` — human-readable task title

**Options:**

| Flag | Description |
|------|-------------|
| `--summary <text>` | Task description |
| `--status <status>` | Task status (e.g., `planned`, `in_progress`, `done`) |
| `--priority <level>` | Priority level (e.g., `low`, `medium`, `high`) |
| `--acceptance <text>` | Acceptance criteria |

**Example:**

```bash
layers task create layers docs "Write public documentation" \
  --summary "README, walkthrough, FAQ, CLI reference" \
  --status in_progress \
  --priority high \
  --acceptance "All docs reviewed and linked"
```

---

## `layers task list`

List task records.

**Options:**

| Flag | Description |
|------|-------------|
| `--project <slug>` | Filter by project |
| `--status <status>` | Filter by status |
| `--json` | Emit JSON output |

**Example:**

```bash
layers task list --project layers --status in_progress
```

---

## `layers curated import <file>`

Import JSONL records from an external file into the canonical curated memory store.

**Arguments:**

- `<file>` — path to a JSONL file containing records in the standard envelope format

**Behavior:** Records are merged into `memoryport/curated-memory.jsonl`. See [data-model.md](data-model.md) for the expected record format.

```bash
layers curated import ./exported-records.jsonl
```

---

## `layers refresh`

Re-index the current repository using GitNexus.

**Behavior:**

- Runs `gitnexus analyze` on the workspace root.
- If `.gitnexus/` already exists with embeddings configured, preserves that setting by passing `--embeddings`.
- Flushes/checks MemoryPort through `uc` when available.
- Outputs JSON status on completion.

**Requires:** `gitnexus` on `PATH`.

```bash
layers refresh
```

---

## `layers remember <kind>`

Append explicit workflow memory to dedicated JSONL storage files.

**Kinds:**

### `layers remember plan`

Store a plan artifact.

| Flag | Required | Description |
|------|----------|-------------|
| `--task <name>` | Yes | Task identifier |
| `--file <path>` | Yes | Path to the plan file |
| `--task-type <type>` | No | Type classification |
| `--artifacts-dir <dir>` | No | Directory for related artifacts |
| `--targets <symbols>` | No | Comma-separated GitNexus target symbols |

### `layers remember learning`

Store a learning.

| Flag | Required | Description |
|------|----------|-------------|
| `--summary <text>` | Yes | Summary of the learning |
| `--task-type <type>` | No | Type classification |
| `--artifacts-dir <dir>` | No | Directory for related artifacts |
| `--targets <symbols>` | No | Comma-separated GitNexus target symbols |

### `layers remember trace`

Store a workflow trace.

| Flag | Required | Description |
|------|----------|-------------|
| `--task <name>` | Conditional | Task identifier (required if no `--summary`) |
| `--summary <text>` | Conditional | Trace summary (required if no `--task`) |
| `--task-type <type>` | No | Type classification |
| `--artifacts-dir <dir>` | No | Directory for related artifacts |
| `--targets <symbols>` | No | Comma-separated GitNexus target symbols |

**Storage:** Plans, learnings, and traces are written to `memoryport/council-{plans,learnings,traces}.jsonl` respectively.

---

## `layers council run <task>`

Execute a three-stage council workflow.

**Arguments:**

- `<task>` — description of the task to evaluate

**Stages:**

1. **Gemini** generates options or proposals
2. **Claude** critiques those proposals
3. **Codex** synthesizes a converged recommendation

**Options:**

| Flag | Default | Description |
|------|---------|-------------|
| `--gemini-cmd <cmd>` | `gemini` | Command to invoke the Gemini CLI |
| `--claude-cmd <cmd>` | `claude` | Command to invoke the Claude CLI |
| `--codex-cmd <cmd>` | `codex` | Command to invoke the Codex CLI |
| `--timeout-secs <n>` | — | Per-stage timeout in seconds |
| `--retry-limit <n>` | — | Maximum retries per stage |
| `--artifacts-dir <dir>` | `memoryport/council-runs` | Output directory for run artifacts |
| `--targets <symbols>` | — | Comma-separated GitNexus symbols for structural context |
| `--json` | — | Emit JSON output |

**Artifacts:** Written to `<artifacts-dir>/<run_id>/`:

- `context.txt`, `context.json` — input context packet
- Per-stage prompt and output files
- `run.json` — run metadata (stages, timing, exit codes)
- `convergence.json` — convergence assessment

**Example:**

```bash
layers council run "Design error handling strategy" \
  --gemini-cmd gemini \
  --claude-cmd claude \
  --codex-cmd codex \
  --targets handle_query,build_context \
  --timeout-secs 120
```

---

## `layers council promote <run_id>`

Promote a completed, converged council run into canonical curated memory.

**Arguments:**

- `<run_id>` — the run identifier (directory name under the artifacts dir)

**Options:**

| Flag | Description |
|------|-------------|
| `--project <slug>` | Target project for the promoted record |
| `--artifacts-dir <dir>` | Override the artifacts directory (default: `memoryport/council-runs`) |
| `--dry-run` | Print the record that would be written without writing it |

**Promotion fails if:**

- The run did not complete
- Convergence did not succeed
- The target project does not exist in curated memory
- The run was already promoted

**Example:**

```bash
# Preview first
layers council promote 20260401T120000Z_design-caching --project layers --dry-run

# Then promote
layers council promote 20260401T120000Z_design-caching --project layers
```

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `LAYERS_WORKSPACE_ROOT` | Override workspace root detection (default: nearest `.git` directory or cwd) |

## Configuration Files

| Path | Purpose |
|------|---------|
| `~/.memoryport/uc.toml` | MemoryPort semantic retrieval configuration (used by `uc`) |
| `.gitnexus/meta.json` | GitNexus index metadata (generated, not hand-edited) |

## Integration Reality

Layers currently assumes:

- **GitNexus** is a code-intelligence system reachable via the local `gitnexus` CLI and optionally MCP-backed runtimes.
- **MemoryPort** is a memory system reachable via `uc` and local canonical files.
- **codex-memoryport-bridge** is an optional OpenAI/Codex Responses proxy for memory injection, not a first-class Layers provider and not a raw MCP tool server.
