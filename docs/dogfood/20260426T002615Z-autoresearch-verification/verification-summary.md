# Layers autoresearch verification summary

Date: 2026-04-26 UTC

Verdict: core local autoresearch flow functions as intended for the current research-radar-oriented MVP, but it is not yet a complete research-radar equivalent.

Verified behavior:
- `autoresearch source list --json` returns valid JSON on an empty isolated workspace.
- `autoresearch source add` persists sources with source type and title.
- `autoresearch profile create --json` persists profiles with keywords, negative keywords, and threshold.
- `autoresearch scan-once --json` scores sources against profiles, creates one entry for the positive source, excludes the negative-keyword source, and ignores irrelevant sources.
- Re-running `scan-once` is idempotent by source URL.
- `autoresearch search agent --json` returns the expected relevant finding with score 1.0.
- Focused `autoresearch` and `preflight` tests pass.
- Full `cargo test --workspace --all-targets` passes when `LAYERS_WORKSPACE_ROOT` is unset.
- `git diff --check` passes.

Dogfood artifacts:
- /Users/xxx/layers/docs/dogfood/20260426T002615Z-autoresearch-verification
- /Users/xxx/layers/docs/dogfood/20260426T002647Z-autoresearch-cross-profile-probe

Important caveat found:
- The command uses source-global dedupe. If one source is first scanned under profile A, scanning profile B afterward does not create a profile-specific match for the already-created source. This confirms the current implementation has entries, but not a separate findings/profile-entry relation.

Workspace caveats:
- `cargo clippy --workspace --all-targets -- -D warnings` still fails. Failures include pre-existing lint debt plus new autoresearch/preflight clippy findings.
- Working tree remains broad/dirty and should be split carefully before commit.
