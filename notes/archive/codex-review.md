# Layers Rust Review

## Scope

This review covers the Rust repo in this checkout and compares it to the current Python prototype at `/Users/bri/.openclaw/workspace/tools/layers.py`, because the prototype is not present in this repo but is still the active implementation surfaced by GitNexus during live execution.

## Bottom Line

The Rust binary is not a shell-wrapper façade. It implements the main Layers behaviors directly in Rust: CLI parsing, routing, memory fallback search, semantic result parsing, graph result normalization, context assembly, audit logging, remember flows, and validation all live in [`src/main.rs`](/Users/bri/Documents/GitHub/layers/src/main.rs).

At the same time, it is not yet ready to replace the Python prototype:

- It is mostly a direct port of the Python prototype, not a materially more durable architecture.
- It still depends on external CLIs for the two core providers: `uc` for Memoryport and `gitnexus` for graph retrieval.
- It has at least one serious correctness issue: by default it points at `/Users/bri/.openclaw/workspace`, not the current repo, so the binary can answer about the wrong codebase entirely.
- It regresses some prototype behavior and validation semantics.

## Answers To The Task Questions

### Does the Rust binary implement the core Layers behavior directly in Rust?

Mostly yes.

Implemented directly in Rust:

- CLI commands and dispatch: [`src/main.rs:12`](/Users/bri/Documents/GitHub/layers/src/main.rs:12), [`src/main.rs:66`](/Users/bri/Documents/GitHub/layers/src/main.rs:66)
- Routing heuristics and rationale: [`src/main.rs:163`](/Users/bri/Documents/GitHub/layers/src/main.rs:163)
- Local JSONL load/append and memory fallback ranking: [`src/main.rs:122`](/Users/bri/Documents/GitHub/layers/src/main.rs:122), [`src/main.rs:329`](/Users/bri/Documents/GitHub/layers/src/main.rs:329), [`src/main.rs:394`](/Users/bri/Documents/GitHub/layers/src/main.rs:394)
- Result distillation, decision extraction, and context synthesis: [`src/main.rs:490`](/Users/bri/Documents/GitHub/layers/src/main.rs:490), [`src/main.rs:512`](/Users/bri/Documents/GitHub/layers/src/main.rs:512), [`src/main.rs:571`](/Users/bri/Documents/GitHub/layers/src/main.rs:571)
- Audit logging and explicit `remember`: [`src/main.rs:647`](/Users/bri/Documents/GitHub/layers/src/main.rs:647), [`src/main.rs:695`](/Users/bri/Documents/GitHub/layers/src/main.rs:695)

Not implemented directly in Rust:

- Semantic memory retrieval itself, which is delegated to `uc`: [`src/main.rs:285`](/Users/bri/Documents/GitHub/layers/src/main.rs:285)
- Structural graph retrieval and indexing, which are delegated to `gitnexus`: [`src/main.rs:431`](/Users/bri/Documents/GitHub/layers/src/main.rs:431), [`src/main.rs:477`](/Users/bri/Documents/GitHub/layers/src/main.rs:477), [`src/main.rs:681`](/Users/bri/Documents/GitHub/layers/src/main.rs:681)

That dependency pattern is acceptable if Layers is supposed to orchestrate those durable local tools. It is not acceptable if the goal is to eliminate all external binaries.

### Is it still depending on Python scripts or shell wrappers for routing / synthesis / formatting logic?

No, not in this repo.

The Rust version does not call the Python prototype or shell scripts for routing, synthesis, formatting, audit generation, or remember behavior. Those behaviors are implemented in Rust in [`src/main.rs`](/Users/bri/Documents/GitHub/layers/src/main.rs).

However, the code is still prototype-shaped because it is very close to a literal port of the Python file:

- Rust routing logic mirrors Python almost exactly: [`src/main.rs:163`](/Users/bri/Documents/GitHub/layers/src/main.rs:163) vs [`layers.py:292`](/Users/bri/.openclaw/workspace/tools/layers.py:292)
- Rust context builder mirrors Python almost exactly: [`src/main.rs:571`](/Users/bri/Documents/GitHub/layers/src/main.rs:571) vs [`layers.py:569`](/Users/bri/.openclaw/workspace/tools/layers.py:569)
- Rust validate mirrors Python almost exactly: [`src/main.rs:734`](/Users/bri/Documents/GitHub/layers/src/main.rs:734) vs [`layers.py:751`](/Users/bri/.openclaw/workspace/tools/layers.py:751)

So the Rust port removed Python as a runtime dependency for orchestration logic, but it has not yet meaningfully evolved past the prototype.

### Are Memoryport and GitNexus integrations real, direct subprocess integrations from Rust?

Yes.

- `uc` is invoked directly via `std::process::Command`: [`src/main.rs:292`](/Users/bri/Documents/GitHub/layers/src/main.rs:292)
- `gitnexus status/list/query/analyze` are invoked directly from Rust: [`src/main.rs:418`](/Users/bri/Documents/GitHub/layers/src/main.rs:418), [`src/main.rs:435`](/Users/bri/Documents/GitHub/layers/src/main.rs:435), [`src/main.rs:480`](/Users/bri/Documents/GitHub/layers/src/main.rs:480), [`src/main.rs:682`](/Users/bri/Documents/GitHub/layers/src/main.rs:682)

This is acceptable external-tool integration. I found no evidence that Rust is routing through Python wrappers to reach those providers.

### What parts of the Python prototype are still missing or weaker in the Rust port?

1. Wrong default workspace targeting.

The Rust binary defaults to `/Users/bri/.openclaw/workspace` instead of deriving the current repo root: [`src/main.rs:78`](/Users/bri/Documents/GitHub/layers/src/main.rs:78). In live execution, `layers query` answered using data from that external workspace, including hits for `tools/layers.py`, which proves the Rust binary can inspect the wrong repository by default. That is a replacement blocker.

2. Missing 1200-word evidence cap.

Python enforces a hard cap in `make_context`: [`layers.py:628`](/Users/bri/.openclaw/workspace/tools/layers.py:628). Rust does not enforce any equivalent bound in [`src/main.rs:571`](/Users/bri/Documents/GitHub/layers/src/main.rs:571). That weakens one of the contract’s explicit safety properties.

3. Refresh behavior regressed.

Python conditionally inserts `--embeddings` when refreshing GitNexus: [`layers.py:699`](/Users/bri/.openclaw/workspace/tools/layers.py:699). Rust always runs `gitnexus analyze <workspace_root>` with no equivalent logic: [`src/main.rs:681`](/Users/bri/Documents/GitHub/layers/src/main.rs:681). If embeddings matter for current prototype behavior, Rust is weaker.

4. Graph normalization is less complete.

Python handles `process_symbols` in graph JSON: [`layers.py:534`](/Users/bri/.openclaw/workspace/tools/layers.py:534). Rust only handles `processes` and `definitions`: [`src/main.rs:443`](/Users/bri/Documents/GitHub/layers/src/main.rs:443). That is a functional regression.

5. Validation semantics are inconsistent with real routing.

`handle_validate` expects `memory_smoke` from a direct search and separately runs `handle_query("What did we already decide about Layers?")`, but the router classifies that query as `neither`, so query-time retrieval is refused even while validation expects memory to exist: [`src/main.rs:734`](/Users/bri/Documents/GitHub/layers/src/main.rs:734). Running `LAYERS_WORKSPACE_ROOT=/Users/bri/Documents/GitHub/layers cargo run --quiet -- validate` produced `"ok": false` for exactly this reason.

6. Execution metadata regressed.

Python includes `duration_ms` in the audit record: [`layers.py:664`](/Users/bri/.openclaw/workspace/tools/layers.py:664). Rust dropped it: [`src/main.rs:660`](/Users/bri/Documents/GitHub/layers/src/main.rs:660). Not critical, but it is still a contract regression if auditability matters.

7. Trace validation regressed slightly.

Python requires `--task or --summary` for `kind=trace`: [`layers.py:735`](/Users/bri/.openclaw/workspace/tools/layers.py:735). Rust allows both to be absent and will append a low-value record: [`src/main.rs:715`](/Users/bri/Documents/GitHub/layers/src/main.rs:715).

### What should be fixed next before the Rust version can replace the Python one?

The priority order is:

1. Fix workspace-root resolution so Rust operates on the intended repo by default.
2. Make Rust validation reflect real query behavior and fail on contract regressions deterministically.
3. Restore parity gaps that affect contract safety or output quality: evidence cap, `process_symbols`, refresh embeddings behavior, trace validation, audit duration.
4. Only after parity is solid, decide whether the acceptable long-term boundary is:
   - Rust orchestration + external `uc`/`gitnexus`, or
   - deeper native Rust integrations with those systems.

## Contract Assessment

### What the Rust port already satisfies

- Local-first design intent
- Explicit routing rather than opaque autonomy
- Auditable output structure
- Direct integration with Memoryport and GitNexus as external local tools
- Explicit writeback through `remember`

### What still violates or weakens the contract

- Defaulting to the wrong workspace breaks the basic contract that Layers should reason over the active repo
- Validation does not prove the end-to-end query contract
- Some prototype safety and fidelity behaviors were dropped in the port

## Durability Assessment

The Rust system is more durable than the Python prototype only in the narrow sense that the orchestration logic now lives in a compiled binary instead of a Python script.

It is not yet a materially stronger system architecture because:

- everything still lives in one large file: [`src/main.rs`](/Users/bri/Documents/GitHub/layers/src/main.rs)
- provider contracts are still CLI-output parsing rather than typed interfaces
- there is no test suite beyond the built-in runtime `validate` command
- replacement-blocking behavior is still environment-coupled and prototype-dependent

## Verification Notes

I verified:

- `cargo check` succeeds
- `cargo run --quiet -- --help` succeeds
- `cargo run --quiet -- query ... --json --no-audit` works
- `LAYERS_WORKSPACE_ROOT=/Users/bri/Documents/GitHub/layers cargo run --quiet -- validate` returns `"ok": false`

That last result is important: the repo compiles, but its own validation already says it is not replacement-ready.
