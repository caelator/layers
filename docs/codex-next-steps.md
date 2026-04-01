# Layers Next Steps

## Current State

The highest-risk cleanup items from the earlier review are no longer live:

- workspace targeting already resolves from the current checkout or git root
- the CLI has been split out of the old one-big-file layout into focused modules
- `validate` now separates contract health from replacement readiness
- GitNexus unborn-repo status no longer leaks raw `HEAD` errors into validation output

What still remains is mostly about deterministic readiness and test depth, not obvious correctness regressions.

## Priority 1: Make Replacement Readiness Deterministic

Keep `validate` green for contract checks, but make `"replacement_ready"` reproducible instead of machine-dependent.

Required changes:

- Decide whether replacement readiness should require:
  - seeded local council-memory fixtures
  - a reachable embedding service
  - or an explicit fixture/mock mode for validation
- Add coverage for the current end-to-end memory query so its expectation is intentional.

Definition of done:

- `validate` reports the same replacement-readiness result on any prepared machine.

## Priority 2: Restore Prototype Parity Where It Matters

Bring back the behaviors that the Python prototype still has and the Rust port dropped.

Implement next:

- Re-check parity gaps against the live Rust code before doing more work.
- Remove any items that are already closed:
  - 1200-word context cap is present
  - `process_symbols` normalization is present
  - refresh preserves `--embeddings`
  - audit includes `duration_ms`
- Keep only genuinely open parity misses on this list.

Definition of done:

- For equivalent inputs, Rust and Python produce materially equivalent routing, evidence structure, and audit behavior.

## Priority 3: Add Real Tests

Move beyond `validate` as the only proof mechanism.

Add:

- Unit tests for `route_query`
- Unit tests for memory ranking and low-signal filtering
- Unit tests for graph-status normalization in unborn repos
- Unit tests for graph-output normalization including `process_symbols`
- Golden tests for assembled `<layers_context>` text
- Integration tests with mocked `uc` and `gitnexus` subprocess outputs

Definition of done:

- Contract-level behavior is covered without needing the user’s live Memoryport/GitNexus environment.

## Priority 4: Tighten Remaining Boundaries

The large-file breakup is mostly done. The remaining structural work is smaller:

- keep validation-specific logic from accreting back into `main.rs`
- consider separating provider probing from validation reporting if `commands.rs` grows again
- add tests next to the modules that own the logic they verify

## Priority 5: Decide The Long-Term Boundary

Make an explicit architecture decision on provider integration.

Acceptable near-term boundary:

- Layers remains a Rust orchestrator
- `uc` and `gitnexus` remain external local tools
- subprocess interaction is wrapped behind typed provider interfaces

Higher-investment boundary:

- replace CLI parsing with direct library/API integrations if those systems expose stable interfaces

Recommendation:

- Keep `uc` and `gitnexus` as acceptable external tools for now
- eliminate only prototype leftovers and contract regressions first
- revisit native integrations after replacement parity is proven

## Replacement Gate

Do not replace the Python prototype until all of these are true:

- default workspace targeting is correct
- `validate` passes in a correctly prepared repo
- parity gaps listed above are closed
- Rust has automated tests for routing, normalization, and context assembly
- running equivalent Rust and Python queries produces the same contract-level outputs
