<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **layers** (25328 symbols, 41886 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/layers/context` | Codebase overview, check index freshness |
| `gitnexus://repo/layers/clusters` | All functional areas |
| `gitnexus://repo/layers/processes` | All execution flows |
| `gitnexus://repo/layers/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->

# Hermes autonomy contract for Layers

## Mission

Build Layers as a local-first Rust context compiler/context spine for coding agents. Prefer small, verified slices that improve evidence collection, retrieval quality, benchmark reliability, or developer ergonomics.

## Standing permissions

Agents may autonomously:
- edit code, docs, tests, benchmark manifests, local scripts, and local Hermes project files inside this repository;
- run Rust verification (`cargo fmt`, `cargo check`, `cargo test`, `cargo clippy`), local benchmark/data-collection commands, packet validation, secret scans, and artifact finalizers;
- create local branches/worktrees, stash/revert recoverable local changes, and clean logs/caches/temp files;
- commit local verified slices when all gates pass, using commit messages prefixed with `[verified]`;
- generate local reports under `.hermes/reports/` or `docs/dogfood/`.

Agents must get explicit approval before:
- pushing to upstream branches, modifying protected branches remotely, or opening/merging PRs;
- deleting non-recoverable user data outside repo temp/cache/build artifacts;
- touching production services or spending new cloud money;
- publishing artifacts that may contain secrets or private data.

## Required development loop

1. Understand scope with GitNexus first: query/context/impact before editing symbols.
2. Use TDD when changing behavior: add or identify a failing test before the implementation when feasible.
3. Keep baseline/eval prompts uncontaminated by Layers context unless the variant explicitly requires it.
4. Preserve exact evidence for benchmark/data-collection work: transcripts, validation logs, packet JSON, diff stats, and binary patches before cleanup.
5. Run independent review for large or fail-closed changes before commit.
6. Run GitNexus change detection before commit.
7. Commit only after verification gates pass.

## Verification gates

Minimum gate for ordinary Rust/code changes:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

For workflow benchmark/training-data changes, also run:

```bash
scripts/hermes/layers-training-data-gate.sh /Users/xxx/layers
```

Convenience wrapper:

```bash
scripts/hermes/layers-verified-gate.sh /Users/xxx/layers
```

## Autonomy/reporting scripts

- `scripts/hermes/layers-autonomy-status.sh /Users/xxx/layers` writes a status report to `.hermes/reports/autonomy-status-latest.md`.
- `scripts/hermes/layers-training-data-gate.sh /Users/xxx/layers` verifies the current Phase 15 mini-batch artifact completeness gate.
- `scripts/hermes/layers-verified-gate.sh /Users/xxx/layers` runs the broader local verification gate.

## Evidence and claim discipline

- Product/effectiveness claims must be evidence-gated against no-Layers baselines.
- Small mini-batches can be used as supervised code-edit training/eval artifacts only when complete diffs and validation evidence are preserved.
- Do not claim product effectiveness until preregistered gates are met, including sufficient paired tasks, sufficient code-heavy tasks, sufficient negative controls, clean packet validation, clean secret scan, and no context-regression signals.

## Secret handling

Do not commit or summarize credentials. Redact encountered secrets as `[REDACTED]` in reports and user-facing summaries.
