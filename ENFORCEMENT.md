# Strict Enforcement

Layers enforces zero warnings at the lint level.

## Local Setup

Hooks live in the tracked `hooks/` directory and are activated via:

```sh
git config core.hooksPath hooks
```

This tells git to run hooks from `hooks/` (which is committed) instead of the local-only `.git/hooks/`. The `pre-commit` and `pre-push` hooks run automatically on every commit and push.

## Local Development

To match CI behavior locally, add to your shell profile (~/.zshrc, ~/.bashrc):

```sh
export RUSTFLAGS="-D warnings"
```

This makes `cargo build`, `cargo test`, and `cargo clippy` fail immediately on any warning.

## Pre-commit / Pre-push Hooks

The `hooks/pre-commit` and `hooks/pre-push` scripts run:

- `cargo clippy -- -D warnings`
- `cargo test`

Commits and pushes are blocked if either fails.

## CI

GitHub Actions runs the same checks on every push/PR. No merge without green CI.
