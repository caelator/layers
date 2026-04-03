# Development

## Prerequisites

- Rust 1.85 or newer
- `gitnexus` on `PATH` for graph-backed workflows
- `uc` and `~/.memoryport/uc.toml` for Memoryport semantic retrieval

## Common Commands

Build:

```bash
cargo build
```

Test:

```bash
cargo test
```

Validate:

```bash
cargo run -- validate
cargo run -- validate --routing benchmarks/routing-answer-keys.jsonl
```

CI-equivalent validation:

```bash
cargo build --release
cargo test
./target/release/layers validate --routing benchmarks/routing-answer-keys.jsonl --ci
```

Inspect help:

```bash
cargo run -- --help
```

## Refreshing GitNexus

Layers exposes a wrapper:

```bash
cargo run -- refresh
```

Equivalent direct command:

```bash
gitnexus analyze .
```

If the repo already has embeddings configured, keep using `--embeddings` when refreshing. Layers tries to preserve that behavior automatically.

## Testing Philosophy

If a behavior matters, it should be exercised by Rust tests or by `validate`.

Current validation covers:

- routing sanity
- routing benchmark pass/fail enforcement for CI
- graph provider reachability
- graph workflow retrieval
- memory workflow retrieval
- typed-memory brief assembly
- project record shape checks
- council command configuration shape

`validate` is useful, but it is not a substitute for focused unit tests.

## Working With Generated Files

Do not commit local runtime noise such as:

- audit logs
- council traces
- council plans
- council run directories
- local `.gitnexus/` state

Canonical curated records are different and may be intentionally versioned.
