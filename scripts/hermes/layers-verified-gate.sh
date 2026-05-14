#!/usr/bin/env bash
set -euo pipefail

repo="${1:-/Users/xxx/layers}"
cd "$repo"

printf '== git status ==\n'
git status --short

printf '\n== rust fmt/check/tests ==\n'
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace

printf '\n== clippy ==\n'
cargo clippy --workspace --all-targets -- -D warnings

printf '\n== diff check ==\n'
git diff --check

printf '\n== finalizer gate, if Phase 15 fixed artifacts exist ==\n'
root="docs/dogfood/20260513T2355Z-phase15-fixed-validpacket-minibatch"
if [ -d "$root" ]; then
  cargo build -q
  ./target/debug/layers workflow-benchmark finalize-run "$root"
else
  echo "skip: $root missing"
fi

printf '\nVERIFIED_GATE_OK\n'
