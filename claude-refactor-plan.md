# Layers Rust Refactor Plan

## Current State

`src/main.rs` is a single 1175-line file containing CLI parsing, routing logic, memory search, graph queries, context synthesis, command handlers, utilities, and tests. The behavior contract (validate) passes. This refactor splits the file into modules without changing any behavior.

---

## Proposed Modules

| File | Responsibility | Approximate lines |
|------|---------------|-------------------|
| `src/main.rs` | CLI struct, `main()` dispatch, `Commands` enum | ~50 |
| `src/types.rs` | `RouteDecision`, `MemoryHit` structs | ~30 |
| `src/config.rs` | Path helpers: `workspace_root`, `find_git_root`, `memoryport_dir`, `audit_path`, `uc_config_path`, `council_files`, `dirs_home`, `iso_now`, `which` | ~80 |
| `src/util.rs` | Pure utilities: `compact`, `append_jsonl`, `load_jsonl`, `tokenize`, `run_command` | ~70 |
| `src/routing.rs` | `score_patterns`, `route_query` (the pattern tables + decision logic) | ~140 |
| `src/memory.rs` | `search_memory_semantic`, `search_memory_fallback`, `search_memory`, `summarize_record`, ranking/scoring helpers (`memory_kind_weight`, `artifact_weight`, `memory_hit_rank`, `looks_low_signal_memory`) | ~220 |
| `src/graph.rs` | `gitnexus_repo_name`, `gitnexus_indexed`, `normalize_graph_output`, `query_graph`, `existing_embeddings_requested` | ~160 |
| `src/synthesis.rs` | `distill_summary`, `extract_decision_signals`, `synthesize_memory_brief`, `format_memory_hit`, `build_context` | ~200 |
| `src/commands.rs` | `run_query`, `handle_query`, `handle_refresh`, `handle_remember`, `handle_validate` | ~220 |

---

## Migration Order

Each step must leave `cargo test` and `cargo build` green.

### Step 1: `types.rs`
Move `RouteDecision` and `MemoryHit` to `src/types.rs`. These are leaf types with no internal dependencies beyond serde. Every other module will import from here, so extracting them first eliminates circular dependency risk.

- Move structs, add `pub` visibility
- Add `mod types;` and `use types::*;` in `main.rs`
- Run `cargo test`

### Step 2: `config.rs`
Move path/environment helpers (`workspace_root`, `find_git_root`, `memoryport_dir`, `audit_path`, `uc_config_path`, `council_files`, `dirs_home`, `iso_now`, `which`). These depend only on `std` and `chrono`.

- Move functions, make `pub(crate)`
- Update callers in `main.rs` to `use crate::config::*`
- Run `cargo test`

### Step 3: `util.rs`
Move `compact`, `append_jsonl`, `load_jsonl`, `tokenize`, `run_command`. These are pure helpers that depend only on `std`, `serde_json`, and `regex`.

- Move functions, make `pub(crate)`
- Run `cargo test`

### Step 4: `routing.rs`
Move `score_patterns` and `route_query`. Depends on `types::RouteDecision`, `regex`, `serde_json`.

- Move functions and pattern tables
- Run `cargo test` (the four routing tests now test through `routing::route_query`)

### Step 5: `memory.rs`
Move all memory-search logic. Depends on `types`, `config`, `util`.

- Move `search_memory_semantic`, `search_memory_fallback`, `search_memory`, `summarize_record`, `memory_kind_weight`, `artifact_weight`, `memory_hit_rank`, `looks_low_signal_memory`
- Run `cargo test`

### Step 6: `graph.rs`
Move GitNexus integration. Depends on `config`, `util`.

- Move `gitnexus_repo_name`, `gitnexus_indexed`, `normalize_graph_output`, `query_graph`, `existing_embeddings_requested`
- Run `cargo test` (the `normalize_graph_process_symbols` test moves here)

### Step 7: `synthesis.rs`
Move context building and formatting. Depends on `types`, `memory` (for `extract_decision_signals`), `util`.

- Move `distill_summary`, `extract_decision_signals`, `synthesize_memory_brief`, `format_memory_hit`, `build_context`
- Run `cargo test`

### Step 8: `commands.rs`
Move command handlers. Depends on `types`, `config`, `util`, `routing`, `memory`, `graph`, `synthesis`.

- Move `run_query`, `handle_query`, `handle_refresh`, `handle_remember`, `handle_validate`
- `main.rs` now only contains `Cli`, `Commands`, and `main()` dispatching to `commands::*`
- Run `cargo test`

---

## Dependency Graph (after refactor)

```
main.rs
  -> commands
       -> routing   (route_query)
       -> memory    (search_memory)
       -> graph     (query_graph)
       -> synthesis (build_context)
       -> config    (paths, iso_now)
       -> util      (append_jsonl, load_jsonl, compact)
       -> types     (RouteDecision, MemoryHit)

routing   -> types, util (score_patterns uses regex)
memory    -> types, config, util
graph     -> config, util
synthesis -> types, memory (extract_decision_signals), util
```

No circular dependencies. Each module depends only on modules extracted before it.

---

## Tests to Keep Passing

| Test | Current location | Moves to |
|------|-----------------|----------|
| `route_memory_query_correctly` | `main.rs` | `routing.rs` |
| `route_graph_query_correctly` | `main.rs` | `routing.rs` |
| `route_both_query_correctly` | `main.rs` | `routing.rs` |
| `route_local_query_to_neither` | `main.rs` | `routing.rs` |
| `normalize_graph_process_symbols` | `main.rs` | `graph.rs` |
| `find_git_root_from_repo` | `main.rs` | `config.rs` |

Additionally, `cargo run -- validate` (the end-to-end behavior contract) must pass after every step.

---

## What Remains in `main.rs`

```rust
mod types;
mod config;
mod util;
mod routing;
mod memory;
mod graph;
mod synthesis;
mod commands;

use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "layers")]
#[command(about = "Local-first context router for Memoryport + GitNexus")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Query { query: String, #[arg(long)] json: bool, #[arg(long)] no_audit: bool },
    Refresh,
    Remember { kind: String, /* ... */ },
    Validate,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Query { query, json, no_audit } => commands::handle_query(&query, json, no_audit),
        Commands::Refresh => commands::handle_refresh(),
        Commands::Remember { .. } => commands::handle_remember(/* ... */),
        Commands::Validate => commands::handle_validate(),
    }
}
```

---

## Risky Seams

1. **`extract_decision_signals` is used by both `memory.rs` and `synthesis.rs`.** It lives in `synthesis.rs` since that's its primary home, but `format_memory_hit` (also in synthesis) calls it. If it ever needs to move, the dependency is one-directional so it's safe.

2. **`which()` is called from `memory.rs` (for `uc`) and `graph.rs` (for `gitnexus`).** Placing it in `config.rs` keeps it centralized.

3. **Regex compilation in `score_patterns` and `search_memory_semantic`.** These currently `unwrap()` on `Regex::new()`. Not a refactor concern, but worth noting as a future improvement (compile once with `lazy_static` or `std::sync::LazyLock`).

4. **The `council_files()` function returns `&'static str` kind labels** that must stay in sync with match arms in `summarize_record` and `handle_remember`. These string constants could become an enum in a follow-up, but that's behavior-adjacent so it's out of scope here.
