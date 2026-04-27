# ContextPacket v2 Schema

`ContextPacket` is the stable Layers product artifact: bounded, cited context that a coding agent can consume before acting.

This document describes the minimal v2 contract. It intentionally avoids deferred v2.1+ systems such as packet quality scoring, session ledgers, or memory ledger v2.

## Required top-level fields

- `schema_version`: integer schema version. v2 packets use `2`.
- `id`: stable packet identifier for this compile result.
- `workspace_id`: workspace or project identifier.
- `query`: original task/query the packet was compiled for.
- `created_at`: packet creation timestamp in RFC 3339 format.
- `git_ref`: current git ref/commit when known, otherwise `null`.
- `route`: retrieval/compile route such as `query`, `preflight`, or `mcp`.
- `confidence`: routing confidence label.
- `budget`: bounded context budget metadata.
- `sections`: ordered context sections.
- `warnings`: degraded/stale/incomplete/conflicting context warnings.
- `selection_trace`: explanations for why individual items were selected.
- `retrieval`: operational retrieval metadata.
- `provenance`: minimal compilation provenance.

## Budget

`budget` records:

- `max_units`: maximum allowed words/tokens depending on caller.
- `used_units`: units selected before rendering.
- `unit`: unit label, e.g. `words` or `tokens`.
- `truncated`: whether output was truncated.

## Sections and items

Each section has:

- `id`: stable section identifier.
- `title`: human-readable title.
- `summary`: optional summary.
- `items`: selected context items.

Each item has:

- `id`: stable item identifier.
- `title`: human-readable title.
- `body`: selected text/snippet.
- `source`: citation object.
- `score`: optional relevance/confidence score.
- `token_estimate`: estimated unit cost.
- `selected_reason`: why the compiler selected this item.
- `tags`: optional labels.

Every v2 item must keep both a non-empty `source.kind`/`source.uri` citation and a non-empty `selected_reason`.

## Source citations

`source` records:

- `kind`: source adapter/type such as `workspace`, `memory`, `gitnexus`, `git`, `autoresearch`, `file`, or `manual`.
- `uri`: source URI/path/record id.
- `repo_path`: optional repo-relative path.
- `line_range`: optional line range.
- `commit`: optional git commit/ref.

## Selection trace

`selection_trace` is a compact, agent-facing explanation list. Each entry records:

- `item_id`: selected item id.
- `reason`: why the item was selected.

Packet finalization may rebuild this from sections to keep sections as the source of truth.

## Retrieval metadata

`retrieval` records:

- `memory_source`: memory backend/source label.
- `memory_latency_ms`: memory retrieval latency.
- `graph_latency_ms`: graph/code retrieval latency.
- `fallback_reason`: optional degraded-mode reason.

## Provenance

`provenance` records how the packet was compiled:

- `compiler`: compiler implementation name.
- `compiler_version`: compiler/schema implementation version.
- `surface`: command or integration surface such as `query`, `preflight`, or `mcp`.
- `workspace_id`: workspace/project identifier used during compilation.
- `git_ref`: git ref/commit observed during compilation, when known.
- `generated_at`: provenance timestamp in RFC 3339 format.
- `source_adapters`: source adapter labels consulted or represented in the packet.

Task 1.1 requires the field to exist with stable defaults. Task 1.2 populates it from packet finalization.

## Compatibility fields

The Rust type still serializes transitional compatibility fields for existing query JSON consumers:

- `task`
- `low_confidence_fallback`
- `scores`
- `why_retrieved`
- `why_not_retrieved`
- `evidence`
- `open_uncertainty`
- `retrieval_meta`

These fields are compatibility surfaces, not the core v2 product contract.

## Minimal example

See `docs/examples/context-packet-v2-minimal.json`.
