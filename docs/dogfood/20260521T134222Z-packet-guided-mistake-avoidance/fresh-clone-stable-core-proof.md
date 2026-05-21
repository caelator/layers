# Fresh Clone Stable-Core Proof

Proof root: /tmp/layers-fresh-clone-proof.bh9VPL/layers
Base: ba3664c
Applied current worktree patch: yes

## Commands
- cargo check --no-default-features --all-targets
- cargo clippy --no-default-features --all-targets -- -D warnings
- cargo run --quiet --no-default-features -- packet validate docs/examples/context-packet-v2-minimal.json
- cargo run --quiet --no-default-features -- packet inspect docs/examples/context-packet-v2-minimal.json
- cargo run --quiet --no-default-features -- packet render docs/examples/context-packet-v2-minimal.json --format objective-brief

## Result
PASS

## Inspect excerpt
ContextPacket inspection
schema_version: 2
id: ctx-example-minimal-v2
workspace_id: layers
query: What should I know before editing README?
created_at: 2026-04-27 00:00:00 UTC
git_ref: none
route: preflight
confidence: high
budget: 6/1200 words (truncated: false)
provenance.compiler: layers-context-packet
provenance.compiler_version: 2
provenance.surface: preflight
provenance.generated_at: 2026-04-27 00:00:00 UTC
provenance.source_adapters: workspace
sections: 1
items: 1
warnings: 0
degraded: false
low_confidence_fallback: false
