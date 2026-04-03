# uc Retrieve Contract

Layers calls `uc retrieve` to perform semantic retrieval against MemoryPort. This document specifies the expected interface so changes to `uc` can be validated against Layers' adapter.

## Command Signature

```
uc -c <config_path> retrieve <query> --top-k <N>
```

- `<config_path>`: Path to uc config file (typically `~/.memoryport/uc.toml`).
- `<query>`: Free-text search query.
- `--top-k <N>`: Maximum number of results to return (Layers defaults to 5).

## Expected Output (stdout)

On success (`exit 0`), `uc` writes results to stdout. Each line is a text chunk returned by the semantic index. Empty stdout (with exit 0) means no matching results — this is valid, not an error.

Example:

```
Council v2 design decision: adopt 4-stage pipeline with convergence detection
Router hardening: added 4 structural signals for graph-intent queries
```

## Error Behavior

| Exit Code | Meaning | Layers Behavior |
|-----------|---------|-----------------|
| 0 | Success (results or empty) | Use results; if empty, fall back to local JSONL |
| Non-zero | Failure | Log stderr to audit trail, fall back to local JSONL |

On failure, `uc` writes diagnostic information to stderr. Layers captures this for audit logging but does not parse it.

## Timeout

Layers enforces a configurable timeout (default 500ms, override via `LAYERS_UC_TIMEOUT_MS`). If `uc` does not exit within this window, Layers kills the process and falls back to local JSONL retrieval.

## Minimum Results Threshold

If `uc` returns fewer than `LAYERS_UC_MIN_RESULTS` (default 1) results, Layers boosts local JSONL results in the interleaved output.

## Config Discovery

Layers checks for the `uc` binary via PATH and for the config file at `~/.memoryport/uc.toml`. If either is missing, semantic retrieval is skipped silently (not an error).

## Store Contract

```
uc -c <config_path> store <text>
```

Used by `layers council promote` to persist distilled learnings back to MemoryPort. Same exit code semantics as `retrieve`.
