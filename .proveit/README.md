# proveit

`proveit` is the executable proof gate for feature completion in `layers`.

Layout:

- `manifests/<feature>.toml` — per-feature proof requirements
- `artifacts/<feature>/<proof>/...json` — stored proof runs
- `verdicts/<feature>.json` — latest computed verdict snapshot

Typical commands:

```bash
cargo run --bin proveit -- verify layers-critical-path-routing
cargo run --bin proveit -- enforce layers-critical-path-routing
cargo run --bin proveit -- report --json
```

Suggested `openclaw-pm` gate command:

```bash
cargo run --manifest-path /Users/bri/Documents/GitHub/layers/Cargo.toml --bin proveit -- enforce <feature-id> --json
```
