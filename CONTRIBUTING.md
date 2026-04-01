# Contributing

## Scope

Keep Layers small. This repo is a local-first CLI that composes Memoryport and GitNexus. It is not a generic workflow platform.

Prefer incremental improvements to:

- routing quality
- durable local record handling
- provider boundary clarity
- council workflow reliability
- validation accuracy
- docs and operational honesty

Avoid introducing large subsystems just for polish.

## Development Loop

```bash
cargo build
cargo test
cargo run -- validate
```

If you change anything that depends on the GitNexus index, refresh it:

```bash
gitnexus analyze --embeddings .
```

Only keep `--embeddings` if the repo already uses embeddings. Layers also preserves that behavior when you run `layers refresh`.

## Repository Layout

- `src/`: Rust CLI implementation
- `memoryport/`: canonical local records plus generated runtime artifacts
- `docs/`: durable public docs

## External Tools

Layers shells out to local tools.

- `gitnexus`: graph search, context, impact, index refresh
- `uc`: Memoryport semantic retrieval

If you change provider behavior, document the operational effect in the README and `docs/`.

## Validation Expectations

Before sending changes upstream:

1. `cargo build`
2. `cargo test`
3. `cargo run -- validate`

If a test or validation check fails, fix the cause rather than documenting around it.
