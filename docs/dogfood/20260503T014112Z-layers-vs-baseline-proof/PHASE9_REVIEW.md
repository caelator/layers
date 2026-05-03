# Phase 9 Artifact/static Scan and Report Review

Run directory: `docs/dogfood/20260503T014112Z-layers-vs-baseline-proof`
Reviewed after commit: `849ed85 [verified] Add phase 8 benchmark artifact run`

## Verdict

PASS for Phase 9 artifact credibility and safety as a deliberately `not_supported` protocol/artifact run.

FAIL / NOT SUPPORTED for any product-performance claim. The artifact set must not be used as evidence that Layers improves real coding workflow outcomes, because independent code-heavy baseline and targeted-preflight implementation runs were not executed.

## Local validation checks

Commands and checks run during Phase 9:

- Parsed `compare/workflow-runs.jsonl` as JSONL.
- Re-ran analyzer in JSON mode:
  - `cargo run -q -- workflow-benchmark analyze docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/compare/workflow-runs.jsonl --json`
- Re-ran analyzer in human mode:
  - `cargo run -q -- workflow-benchmark analyze docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/compare/workflow-runs.jsonl`
- Re-ran task validation:
  - `cargo run -q -- workflow-benchmark validate-tasks docs/dogfood/20260503T014112Z-layers-vs-baseline-proof/tasks`
- Secret-scanned the artifact directory for live secret shapes:
  - OpenAI-style `sk-*` keys
  - GitHub `gh[pousr]_...` tokens
  - Slack `xox...` tokens
  - AWS access keys
  - assignment-style `api_key`, `secret`, `password`, `passwd`, and `token` values

Results:

- Run records: 62 rows.
- Tasks: 31.
- Variants: exactly `baseline` and `layers_targeted_preflight`.
- Incomplete task pairs: 0.
- Baseline Layers-overhead contamination: 0 rows.
- Broad-query or MCP-preflight rows mixed into targeted-preflight comparison: 0.
- Baseline transcripts: 31.
- Targeted-preflight transcripts: 31.
- Targeted-preflight packet JSON artifacts: 25.
- Targeted-preflight negative-control abstention artifacts: 6.
- Analyzer JSON exit: 0.
- Analyzer human exit: 0.
- Task validation exit: 0.
- Actionable secret findings: 0.

## Independent review summary

An independent reviewer inspected the Phase 8 artifact directory, Phase 9 plan checklist, `CLAIM.md`, `SUMMARY.json`, `SECRET_SCAN.md`, `workflow-runs.jsonl`, reports, task specs, transcripts, validation logs, packet artifacts, and packet validation/inspection logs.

### Blockers

- Product-performance claim remains blocked / not supported. The artifacts do not support “Layers improves real coding workflow success/time/tokens” because code-heavy task-solving was intentionally not executed. All 25 code-heavy task rows are scored 0.0 for both variants with validation marked not executed. `CLAIM.md` correctly states this.
- Analyzer claim thresholds are weaker than the preregistered `CLAIM_GATES.md` in at least one visible way: analyzer JSON reports `min_paired_tasks: 1`, while `CLAIM_GATES.md` requires at least 30 paired tasks and 20 code-heavy paired tasks. This did not affect this run's final status because other hard blockers already make the claim `not_supported`, and the run has 31 paired tasks, but it should be fixed before relying on analyzer status for a future effectiveness run.

### Warnings

- Task corpus cherry-picking risk is mitigated but not eliminated. The claim gates and corpus landed before the Phase 8 artifact commit, and the artifact task snapshot matches the benchmark task corpus, but the corpus is repo-internal and self-referential. Treat it as narrow dogfood evidence, not broad external proof.
- Broad-query text appears in targeted-preflight packet artifacts, but review indicates it is not accidentally mixed as a broad-query evidence surface. Hits are task text, guardrail text warning not to pool variants, source excerpts, or validation command names.
- Negative-control targeted-preflight rows record some context-relevant tokens despite explicit abstention artifacts. Since unnecessary context injections are 0 and abstention artifacts are present, this appears to be token-accounting noise rather than actual injected context. Clean this up for metric clarity before the next effectiveness run.
- Stale-context traps were scored harshly only because code-heavy tasks were unexecuted and failed. The run did not actually test whether stale-memory use would be penalized during an implementation attempt.

### Positives

- Every task has paired baseline and targeted-preflight runs.
- No baseline run appears to have used Layers context.
- Every non-abstained targeted-preflight run saved preflight packet artifacts.
- Every negative control saved an explicit abstention artifact.
- Packet validation/inspection artifacts parse and are recorded for generated packets.
- Negative controls are handled credibly: all six validate successfully, targeted preflight abstention rate is 1.0, and unnecessary context injection rate is 0.0.
- Success scoring is based on validation state, not vibes: negative controls have validation logs; code-heavy tasks have not-executed logs and are scored 0.0.
- No live credentials were found by the Phase 9 secret scan.
- `CLAIM.md` does not overclaim. It explicitly says the product-performance claim is not supported and limits the artifact value to protocol validation, reporting plumbing, negative-control abstention, and targeted-preflight packet capture.

## Required follow-up before a real effectiveness claim

1. Build an automated benchmark runner that launches isolated coding-agent sessions per task and variant in throwaway worktrees.
2. Capture machine-readable transcripts, validation logs, edits, timing, token/context accounting, and run records from actual implementation attempts.
3. Align analyzer claim thresholds with preregistered `CLAIM_GATES.md`, including minimum paired tasks and minimum code-heavy paired tasks.
4. Clean up negative-control token accounting so abstained runs do not look like they consumed relevant context.
5. Rerun the same preregistered corpus without moving gates unless the gates become stricter.
