# Codex Phase III Summary

## Landed

- Added a Layers-native `council run` workflow that keeps the model order fixed as Gemini -> Claude -> Codex.
- The new workflow retrieves Layers context first, optionally enriches the run with GitNexus impact metadata for `--targets`, then writes a durable run directory with:
  - `context.txt`
  - `context.json`
  - per-stage prompt files
  - per-attempt stdout/stderr artifacts
  - `run.json`
  - `convergence.json`
- Added typed council run records in Rust for stage attempts, stage state, convergence, and run state.
- Added retry and timeout discipline per stage:
  - stages are marked `running`, `retrying`, `failed`, or `succeeded`
  - each attempt records pid, exit code, duration, and artifact paths
  - empty output is treated as a failed attempt
- Added convergence recording after the Codex stage and automatic trace writeback into `memoryport/council-traces.jsonl`.
- Extended `validate` to report the fixed council role order and whether live council commands are configured.
- Added deterministic tests for:
  - successful 3-stage execution
  - retry after a transient stage failure
- Hardened env-sensitive tests with a shared workspace lock so `cargo test` stays green under the default parallel test runner.

## Verified

- `cargo test`
- `cargo run -- validate`
- One local smoke run of `cargo run -- council run ...` with shell-based fake Gemini/Claude/Codex commands to verify the end-to-end CLI path, then removed the synthetic trace record from `memoryport/council-traces.jsonl`.

## What Landed In Code

- New council engine: `src/council.rs`
- New CLI surface: `layers council run`
- New typed records: `CouncilStageAttempt`, `CouncilStageRecord`, `CouncilConvergenceRecord`, `CouncilRunRecord`
- New shared test helper: `src/test_support.rs`

## Still Later

- No built-in provider adapters for real Gemini/Claude/Codex clients yet; the workflow currently shells out through explicit commands or environment variables.
- No promotion path yet from a completed council convergence into canonical curated memory records such as decisions, constraints, or next steps.
- No separate handoff/postmortem workflow commands yet.
- No long-running background supervision beyond the persisted run-state and per-stage timeout/retry loop.

## Notes

- GitNexus impact analysis was run before editing the existing touched symbols:
  - `handle_remember`: LOW
  - `handle_validate`: LOW
  - `main`: LOW
- The AGENTS.md `gitnexus_detect_changes()` requirement could not be executed here because that MCP tool is not exposed in this Codex environment.
