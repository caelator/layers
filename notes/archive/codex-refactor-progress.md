# Codex Refactor Progress

## Current checkpoint

Completed the next safe reduction of `src/main.rs` after re-verifying live behavior:

- Confirmed `cargo test` is still green.
- Confirmed `cargo run -- validate` is green again at the contract level:
  - top-level `"ok"` now reflects routing/audit/graph contract health
  - `"replacement_ready"` stays false when the local environment cannot supply memory evidence
- Added `src/commands.rs` and moved command orchestration into it:
  - `run_query`
  - `handle_query`
  - `handle_refresh`
  - `handle_remember`
  - `handle_validate`
- Kept CLI parsing and subcommand dispatch in `src/main.rs`.
- Left behavior intentionally unchanged apart from module boundaries and import cleanup.

`src/main.rs` is now down to the CLI entrypoint plus tests.

## GitNexus impact analysis run before edits

Ran upstream impact analysis on the symbols touched by the handler extraction, all against `--repo layers`:

- `run_query`: `LOW` risk
  - Direct callers: `handle_query`, `handle_validate`
  - Indirect caller: `main`
  - Affected processes: `main`, `handle_validate`
- `handle_query`: `LOW` risk
  - Direct caller: `main`
- `handle_validate`: `LOW` risk
  - Direct caller: `main`
- `handle_refresh`: `LOW` risk
  - Direct caller: `main`
- `handle_remember`: `LOW` risk
  - Direct caller: `main`
- `main`: `LOW` risk
  - No upstream dependents in the indexed graph

Because the blast radius was small and local to the CLI entrypoint, this slice was safe to extract without semantic changes.

## Verification

- `cargo test`: passes
- `cargo run -- validate`: returns top-level `"ok": true`

`validate` still reports `"replacement_ready": false` in this environment because semantic retrieval cannot reach the local embedding endpoint (`http://localhost:11434/api/embed`), and the repo does not currently have council memory data that satisfies the end-to-end memory query without embeddings. That is now reported as an environment-readiness limitation instead of a binary-level contract failure.

## Notes

- The installed `gitnexus` CLI in this environment does not expose `detect_changes`, so I could not run the AGENTS.md-prescribed post-refactor scope command directly.
- `cargo run -- validate` no longer prints the raw `fatal: ambiguous argument 'HEAD'` noise when the repository has no commits yet. The status path now summarizes that condition plainly.

## What remains

Next safe slices, in order:

1. Decide whether to seed council memory fixtures for deterministic offline validation, or explicitly require a live embedding service for replacement readiness.
2. Add integration tests with mocked `uc` and `gitnexus` subprocess outputs so validate-path behavior is covered without depending on the local machine state.
3. If behavior stays stable, continue splitting validation-specific assertions away from CLI smoke coverage.
