# Dogfood Postmortem

Run: `20260425T174712Z-layers-production-readiness`

## Result

Partially helpful, but not production-ready as a context compiler yet.

Layers successfully produced a valid ContextPacket v1 artifact and an agent-prompt rendering for its own production-readiness task. That proves the basic schema/rendering path is usable. The content quality did not meet the bar for production readiness because the packet failed to surface several decisive facts from the live repo state.

## What Layers Got Right

- `layers query --json --no-audit` exited successfully.
- `context-packet.json` parsed with `python3 -m json.tool`.
- `schema_version` was `1`.
- Transitional compatibility fields were present for old JSON consumers.
- `layers query --agent-prompt --no-audit` exited successfully.
- Packet included a warning that memory quality had low relevance.
- `layers validate` exited 0 in this workspace.

## What Layers Got Wrong

- It routed a code-heavy production-readiness query to `memory_only` with `high` confidence.
- It included a low-relevance memory warning but did not lower confidence or force graph/code fallback.
- It did not surface current dirty working tree state.
- It did not surface the current clippy failure set.
- It did not include GitNexus/code graph context for the ContextPacket refactor.
- It did not cite the key files under active change.
- It did not produce concrete next validation commands as first-class packet content.

## Product Fixes Implied by This Run

1. Add workspace state metadata/warnings:
   - branch
   - head commit
   - dirty status
   - untracked files count/list when small

2. Tighten confidence semantics:
   - `confidence=high` should not survive a low relevance warning without additional high-quality evidence.

3. Improve route selection for code-production tasks:
   - queries mentioning refactor, clippy, tests, files, symbols, MCP, validation, or production readiness should generally route to `both` or trigger graph fallback.

4. Add validation commands as ContextPacket content:
   - `cargo test --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `git diff --check`
   - JSON parse check for packet output

5. Add eval fixture from this dogfood run:
   - expected hits: `crates/layers-core/src/context_packet.rs`, `src/cmd/query.rs`, `src/main.rs`, `src/cmd/validate.rs`, `docs/NORTH_STAR.md`, `docs/PRODUCTION_READINESS_DOGFOOD_PLAN.md`
   - expected warnings: dirty tree, low memory relevance when applicable
   - forbidden behavior: memory-only high-confidence packet for code-heavy production-readiness task

## Candidate Durable Memories to Promote

Do not promote raw run details. Promote only after the corresponding product change lands.

Candidate:

```json
{"kind":"learning","summary":"Layers dogfood showed ContextPacket v1 JSON can be valid while retrieval quality is still insufficient; production packets need dirty-tree warnings, confidence tied to retrieval quality, graph fallback for code-heavy tasks, and validation-command sections.","source":"docs/dogfood/20260425T174712Z-layers-production-readiness/"}
```

## Follow-Up Tasks

1. Add dirty-tree metadata/warnings to `ContextPacket` construction.
2. Make low relevance downgrade confidence or trigger graph fallback.
3. Add validation command section to ContextPacket v1.
4. Add context-quality eval fixture for this dogfood query.
5. Decide and document clippy release-gate policy.
6. Split and commit the current ContextPacket refactor before expanding scope.
