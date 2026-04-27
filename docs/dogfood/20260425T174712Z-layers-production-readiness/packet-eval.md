# ContextPacket Evaluation

Run: `20260425T174712Z-layers-production-readiness`

Query:

> Dogfood Layers on its own repo for production readiness. Focus on ContextPacket v1 refactor under review, cargo test passing, clippy pedantic/test/doc lint debt, degraded provider warnings, explicit memory, GitNexus impact context, and next validation commands.

## Summary

Did the packet help? Partial.

The current `layers query --json` path produced valid ContextPacket v1 JSON and preserved transitional compatibility fields. The packet was machine-parseable and the agent prompt renderer ran successfully. However, the actual retrieved context was too memory-only and not specific enough for the production-readiness task: no GitNexus/code section appeared, the packet did not surface the current dirty working tree, and it did not mention the just-observed clippy failure set.

## Command Results

- `context-packet.exit`: 0
- `context-packet-json-tool.exit`: 0
- `context-packet.agent-prompt.exit`: 0
- `validate.exit`: 0

## Packet Shape

Observed from `context-packet.json`:

- `schema_version`: 1
- `route`: `memory_only`
- `confidence`: `high`
- sections: one `Memory` section with 3 items
- selection trace entries: 3
- warnings:
  - `retrieval_warning`: Memory quality: low relevance — query terms rarely appear in results

Legacy compatibility fields present:

- `task`
- `scores`
- `why_retrieved`
- `why_not_retrieved`
- `evidence`
- `open_uncertainty`
- `retrieval_meta`

## Packet Quality Scores

- JSON/schema validity, 0-10: 10
- Task relevance, 0-15: 7
- Grounded citations, 0-15: 6
- Selection reasons, 0-10: 6
- Warning/degraded-mode honesty, 0-10: 6
- Graph usefulness, 0-10: 0
- Memory usefulness, 0-10: 5
- Actionability/validation commands, 0-10: 4
- Concision/budget fit, 0-5: 5
- Agent-readiness, 0-5: 4

Packet quality total: 53/100

## Critical Misses

- Missing current dirty-tree / uncommitted broad refactor warning.
- Missing current `cargo clippy --workspace --all-targets -- -D warnings` failure summary.
- Missing GitNexus/code graph context despite the query explicitly asking for ContextPacket refactor and impact context.
- Missing exact files likely to matter: `crates/layers-core/src/context_packet.rs`, `src/cmd/query.rs`, `src/main.rs`, `src/cmd/validate.rs`, `crates/layers-mcp/*`.
- Did not recommend concrete validation commands beyond what a general agent would infer.

## Best Retrieved Items

1. The packet correctly produced schema version 1 and parseable JSON.
2. It preserved transitional compatibility fields needed for existing JSON consumers.
3. It honestly warned that memory relevance was low.

## Worst / Noisiest Gaps

1. `route=memory_only` and `confidence=high` conflict with the low relevance warning and code-heavy task.
2. No graph/code section appeared for a task that should be at least `both`.
3. Dirty working tree and clippy debt were not included even though those are central production-readiness facts.

## Verdict

Keep the ContextPacket v1 schema/rendering path, but treat this dogfood run as a product-quality failure for retrieval/routing completeness.

Immediate product fixes implied:

1. Add dirty-tree status into packet metadata or warnings.
2. Lower confidence or add degraded warning when quality evaluation says memory relevance is low.
3. Make query routing choose `both` or add graph fallback for code-production tasks mentioning files, refactors, clippy, MCP, or validation.
4. Include suggested validation commands in ContextPacket v1.
5. Add this run as an eval fixture so future packets must retrieve code/graph context for production-readiness tasks.
