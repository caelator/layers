# Dogfood Demo: Packet Prevents Runtime Sprawl

Status: packet-guided demo artifact, not a benchmark success claim.

## Scenario

A coding agent is asked to add stable `layers mcp serve`, `layers impact <target>`, and `layers memory list/search/show` UX while keeping Layers narrowed around the local-first ContextPacket compiler.

This is exactly where prior Layers work has been risky: the agent can drift into generic runtime expansion, expose filesystem/process/subagent tools through MCP, or accidentally commit local telemetry artifacts.

## Packet Artifact

- Full packet JSON: `context.packet.json`
- Compact agent handoff: `objective-brief.md`
- Validation report: `packet-validate.txt`

The packet is intentionally produced by the current `layers preflight --json` path and rendered through `layers packet render --format objective-brief`, so the demo exercises the same artifact surface expected from real agent runs.

## What the Packet Should Prevent

1. Runtime sprawl
   - The packet cites the repo north-star and product contract targets.
   - Stable MCP must expose context compiler tools only, not generic runtime/process/filesystem/subagent capabilities.

2. Wrong validation path
   - The packet points the agent toward Rust verification gates, including workspace tests/clippy/fmt.
   - It avoids treating docs-only validation as enough for CLI/MCP behavior changes.

3. Telemetry artifact commits
   - The packet includes workspace/dirty-state context and warnings that local generated files must be inspected before handoff.
   - Local telemetry churn should be restored rather than included in product commits.

4. Overlarge prompt injection
   - The agent-facing handoff is `objective-brief.md`, not the full packet body.
   - The full JSON remains available as a cited artifact for audit and validation.

## Evidence Included

The full packet includes sections for:

- workspace state
- repo-owned context policy
- curated memory
- autoresearch findings when available
- code targets
- impact context
- validation commands
- preflight summary

The objective brief includes only the bounded handoff: objective, constraints, citations, validation plan, and insufficiency guard.

## Validation Outcome

`packet-validate.txt` shows the packet is schema-valid in non-strict mode. Strict mode is deliberately not claimed here because current preflight packets can contain warnings/degraded context; this demo is evidence of useful packet guidance, not proof that all product claims pass.

## Claim Boundary

Supported by this demo:

- Layers can compile a real ContextPacket for this repo task.
- Layers can render a compact Objective Brief suitable for a coding agent.
- The packet explicitly calls out likely failure modes: runtime sprawl, validation gaps, and generated artifact risk.

Not supported by this demo alone:

- Layers is better than no context across benchmarked agent runs.
- Layers reduces total tokens end-to-end.
- Layers improves success rate against baseline.

Those claims require the workflow benchmark gate to pass.
