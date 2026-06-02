# Layers v2 Product Contract

## Binding Product Definition

Layers v2 is the local-first `ContextPacket` compiler for coding agents **and**, on dedicated branches, an autospawned, continuously-chained research-and-implementation runtime that drives the same compile/implement/verify cycle autonomously under a configurable cooldown and a soft wall-clock cap.

Its stable jobs are to answer:

> What does this agent need to know before touching this code?

and, when the user is away from the keyboard,

> What useful, evidence-gated work can an autospawned, continuously-chained research runtime do to this codebase, and what is the audit trail?

The first answer is a bounded, cited, reproducible `ContextPacket` that can be consumed by humans, CLI workflows, MCP clients, and coding agents before they edit a repository. The second answer is a reviewable TSV at `.layers/autoresearch/sweeps.tsv`, a `git log` of `[verified]` commits, a cost ledger, and a dogfood report — produced under the same TDD + verification gate as a human in the loop, on a dedicated branch only.

The 2026-06-01 research-runtime pivot (`.hermes/plans/2026-06-01_layers-research-runtime-pivot.md`) is the binding record of how and why this second job entered the v2 contract. The 2026-06-02 autospawn pivot (`.hermes/plans/2026-06-02_autospawn-continuous-runtime-pivot.md`) is the binding record of how and why the second job's posture widened from "bounded, cronjob-only" to "autospawned, continuously-chained on dedicated branches."

## Core Artifact

`ContextPacket` is the primary core product artifact. The per-iteration TSV produced by the research runtime (`.layers/autoresearch/sweeps.tsv`) is a secondary core artifact: every overnight iteration must produce one, with `sweep_id`, `iteration`, `before_packet_grade`, `after_packet_grade`, `findings_delta`, and a `kept|discarded|crashed|skipped` decision.

A stable v2 packet must preserve:

- task/query text
- workspace identity
- git/worktree state when known
- ordered context sections
- source citations
- selection reasons
- warnings for degraded, stale, incomplete, or conflicting context
- validation guidance when available
- machine-readable JSON rendering
- human-readable Markdown or agent-prompt rendering

A stable v2 sweep row must preserve:

- sweep and iteration identifiers
- started/finished timestamps (RFC 3339)
- the active profile identifier when one is set
- the count of selected findings, missing-context items, and suggested actions
- before/after packet grade when grading is enabled
- a keep/discard/crash/skip decision with a one-line reason

Commands, stores, adapters, and MCP tools are stable only insofar as they feed, compile, validate, render, or expose these artifacts. The research runtime's `program.md`-style instruction file (human-editable) is a stable input, not a hidden parameter.

## Allowed Stable-Core Feature Jobs

A v2 feature belongs in stable core only if it does at least one of these jobs:

1. feed a `ContextPacket`
2. compile or finalize a `ContextPacket`
3. render, validate, inspect, or diff a `ContextPacket`
4. store minimal durable local context needed by future packets
5. expose stable context tools to agents through CLI or MCP
6. run a bounded research-and-implementation cycle on a dedicated branch, producing a sweep TSV row and `[verified]` commits under a hard wall-clock cap, and reporting keep/discard/crash per iteration

If a feature does not satisfy one of these jobs, it must be beta, deprecated compatibility, or out of scope.

Job 6 is bounded by the following invariants, all of which are stable-core contract:

- The runtime runs on a dedicated branch (`autoresearch/<tag>` or equivalent), never on a protected branch.
- The runtime is launched either synchronously (`layers research run` in the foreground) or via `cronjob` with `notify_on_complete: true`, or **autospawned** by a configured trigger (heartbeat, file-watch, or daemon pulse). Auto-spawn is permitted in the v2.2 autospawn posture; it is not permitted to recursively schedule additional cronjobs that themselves schedule more cronjobs.
- Wall-clock cap is configurable per run; default cap is 12 hours. The cap is **soft** in the autospawn posture — the runtime is expected to chain back-to-back across runs, gated by a configurable cooldown (default 5 minutes), and to abort on `SIGINT`/`SIGTERM` or unrecoverable error rather than at the cap.
- Every iteration that proposes a code change must pass the v2 verification gate (`cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `git diff --check`) before commit. The verification gate is unconditional across all postures.
- Commits use the existing `[verified]` tag.
- The runtime writes a sweep row per iteration to `.layers/autoresearch/sweeps.tsv`. No silent omissions.
- The runtime has cooperative cancellation (`layers research stop <run-id>`). In the autospawn posture, cancellation drains the in-flight iteration and exits the autospawn loop cleanly without firing the next iteration.
- The autospawn runtime's blast radius is restricted to dedicated branches: it never holds the user's primary session, never auto-commits to `main`/`master`/release branches, and never edits memory outside `.layers/autoresearch/` and the dedicated branch's working tree.

## Non-Goals

Layers v2.0 is not:

- a personal assistant
- a chat-first product
- a hosted service
- a general agent runtime (the v2.0 contract adds a *bounded* research runtime as job 6; the v2.2 autospawn pivot widens that job to autospawned, continuously-chained runs on dedicated branches, gated by a configurable cooldown and a soft wall-clock cap, and **not** permitted on the user's primary session or on protected branches; general/unbounded agent execution on the primary session remains a non-goal)
- a messaging gateway
- a provider abstraction platform
- a generic subagent orchestrator
- a generic tool execution server
- a generic vector database
- a replacement for Hermes, OpenClaw, DeerFlow, Letta, mem0, Graphiti, Cognee, or MemoryPort

Those systems are execution layers, memory backends, or integration peers. Layers should make them better by compiling local coding context, and — under job 6 — by running autospawned, continuously-chained cycles on dedicated branches that produce `[verified]` commits and a reviewable TSV.

## Removed Surfaces

No existing user-facing command is removed in v2.0 solely because of this contract.

Instead, v2.0 distinguishes stable core from deprecated compatibility. Deprecated surfaces may remain available behind default compatibility features, but they are not allowed to block stable-core no-default-feature builds or appear in stable MCP defaults. Future major releases may remove compatibility surfaces after separate migration plans.

## Stable Core Surface

The v2.0 stable core converges on:

| Surface | Status | Contract |
|---------|--------|----------|
| `ContextPacket` schema/renderers | Stable core | Versioned context artifact and renderings |
| `layers query` | Stable core | Compile task context from local memory/retrieval inputs |
| `layers preflight` | Stable core | Compile pre-edit context for code-heavy work and explicit targets |
| `layers packet validate/inspect/render/diff/grade` | Stable core | Treat packets as durable artifacts |
| `layers validate` / `layers doctor` | Stable core | Explain readiness and degraded modes |
| `layers refresh` | Support | Refresh optional derived context such as GitNexus data |
| `layers mcp serve` stable tools | Stable core | Expose context compilation and packet validation to agents |
| `layers memory list/search/show` | Stable core | Inspect existing curated/remembered memory without a new backend |
| `layers impact <target>` | Stable core | Summarize GitNexus-backed or degraded blast-radius context |
| `layers autoresearch sweep` | Stable core | Synchronous bounded research sweep with TSV log; the foundation for job 6 |
| `layers research run` / `status` / `stop` | Stable core (job 6) | Autospawned, continuously-chained research-and-implementation cycle on a dedicated branch, with `--autospawn-cooldown` and `--autospawn-trigger` flags for the v2.2 posture; the bounded-cron posture from v2.1 remains a valid subset |

## Beta / v2.x Expansion Surface

These are strategically aligned, but not required to ship the minimal v2.0 release:

| Surface | Target | Reason |
|---------|--------|--------|
| session import/distillation | v2.2 | Cross-agent continuity from transcripts |
| packet quality scoring | v2.2 | Measurable packet usefulness after dogfood evidence |
| autoresearch freshness/network fetchers | v2.3 | External context intake once compiler core is stable |
| release packaging | v2.x | Distribution polish after core semantics settle |

## Deprecated Compatibility Surface

These surfaces may remain available for compatibility or experiments, but must not drive v2 architecture:

| Surface | Status | Rule |
|---------|--------|------|
| `layers daemon` | Deprecated / compatibility | Default-compatible, not stable-core required |
| `layers chat` | Deprecated / experimental | Must not become primary UX |
| monitor / technician automation | Deprecated / experimental | Only useful when scoped to context health |
| provider runtime | Deprecated / compatibility | Do not expand as a platform |
| messaging channels | Deprecated / compatibility | Do not expand as product surface |
| generic tool runtime | Deprecated / compatibility | Do not expose through stable MCP defaults |
| subagent orchestration | Deprecated / compatibility | Do not compete with mature agent frameworks |
| infrastructure credential management | Deprecated / experimental | Outside context compiler thesis |

## Stable-Core Build Boundary

The stable target set must continue to compile without deprecated runtime and compatibility-storage defaults:

```bash
cargo check --no-default-features --all-targets
cargo clippy --no-default-features --all-targets -- -D warnings
```

Default features may preserve compatibility, but no-default stable-core checks are the guardrail against accidental runtime or storage gravity.

## Stable MCP Contract

Stable MCP defaults must expose only product-facing context tools. They must not expose generic filesystem, process, daemon, subagent, or mutating runtime capabilities.

Allowed stable MCP tool classes:

- context compilation
- preflight context compilation
- packet validation
- read-only memory retrieval where implemented
- read-only impact/context analysis where implemented

Generic runtime tools require explicit opt-in outside the stable context surface.

## Release Boundary for v2.0

Layers v2.0 is complete when:

- `ContextPacket` v2 minimal schema is documented and tested
- `query` and `preflight` compile packets through a shared compiler path
- packet validate/inspect/render/diff/grade commands exist
- stable MCP context tools call the shared compiler path
- stable-core no-default-feature checks pass in CI
- fresh clone bootstrap is truthful and reproducible for stable core
- at least one dogfood proof demonstrates packet-guided agent work
- (added in the 2026-06-01 pivot) `layers autoresearch sweep` writes a TSV row per iteration with the v2 column schema, and the synchronous sweep slice is on the stable core path
- (added in the 2026-06-01 pivot) the v2 evidence gate for the overnight research runtime is documented and preregistered, even though job 6 itself is a v2.1+ delivery

Memory ledger v2, session distillation, impact engine v2, packet quality benchmarks, autoresearch network fetchers, and the `layers research run` overnight command are v2.1+ expansion, not v2.0 blockers. The synchronous `layers autoresearch sweep` slice at `6de7932` and the v2 sweep-row schema above are the v2.0 deliverable from the pivot direction.

The v2.1 deliverable was the bounded overnight-researcher runtime (`layers research run` with hard wall-clock cap, cronjob-only launch, default 12h cap, abort-on-cap) at commit `5624542`. The v2.2 deliverable is the autospawned, continuously-chained runtime on dedicated branches (`layers research run` with `--autospawn-cooldown` and `--autospawn-trigger`, soft wall-clock cap, configurable cooldown default 5m, abort on `SIGINT`/`SIGTERM` or unrecoverable error) at the commit produced by the implementation slice on `feature/layers-autospawn`.
