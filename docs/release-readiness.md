# Release Readiness

## Current State

The repo builds, tests, and validates cleanly. Public documentation covers the full command surface, data model, getting-started workflow, and common questions.

- `cargo build` passes
- `cargo test` passes
- `cargo run -- validate` passes
- Public docs: README, walkthrough, CLI reference, FAQ, data model, development guide

## What Is Ready

- Local evaluation from source
- Query routing and context assembly
- Curated record import
- Validation reporting
- Public documentation for all commands and workflows

## What Is Experimental

- Council workflow execution depends on external model CLIs being configured
- Semantic Memoryport retrieval depends on `uc` plus its local config
- GitNexus-backed workflows depend on a local index that may become stale
- `validate` treats fallback and typed-memory success as enough for overall pass status even when the embedding backend is unavailable
- Provider contracts (the interface between Layers and external tools) may change

## Public Expectations

This repository should be presented as:

- a local-first CLI
- an orchestration layer over external local tools
- a project that prefers explicit durable records over opaque background state

It should not be presented as:

- a hosted service
- a stable provider API
- a fully mature workflow platform

## Deferred Non-Blocking Work

- Tighten the meaning of `replacement_ready` in `validate`
- Add integration tests for missing-provider and degraded-provider scenarios
- Improve command help text descriptions inside the CLI itself
- Add example JSONL files for curated import if import usage expands
