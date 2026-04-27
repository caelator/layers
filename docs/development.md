# Development

## Prerequisites

- Rust 1.85 or newer
- `gitnexus` on `PATH` for graph-backed workflows
- `uc` and `~/.memoryport/uc.toml` for MemoryPort semantic retrieval

## Bootstrap and Compatibility Dependencies

The stable target set must work without hidden sibling repositories:

```bash
cargo check --no-default-features --all-targets
cargo clippy --no-default-features --all-targets -- -D warnings
```

Default-feature development still enables deprecated compatibility storage through the `substrate-storage` feature. That feature uses the optional git `substrate` dependency:

```toml
substrate = { git = "https://github.com/caelator/substrate.git", optional = true }
```

A fresh clone can run full default-feature builds without placing `substrate` next to `layers`:

```bash
cargo check --workspace --all-targets
```

This is compatibility baggage, not a stable-core requirement. Do not add new stable context-compiler code that depends on `substrate-storage`. The `proveit` binary requires `substrate-storage` and is intentionally skipped by no-default all-target checks.

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

Stable-core validation:

```bash
cargo check --no-default-features --all-targets
cargo clippy --no-default-features --all-targets -- -D warnings
```

The stable-core gate must not require deprecated runtime, daemon, monitor, technician, channel, or hidden sibling-repository dependencies. Default-feature CI may still exercise compatibility surfaces.

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
- curated record shape checks
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
