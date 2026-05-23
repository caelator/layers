# Workflow Benchmark Transcript

Task ID: code-feature-mcp-preflight-stable-registry
Variant: baseline
Run ID: code-feature-mcp-preflight-stable-registry--baseline
Worktree: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/worktrees/code-feature-mcp-preflight-stable-registry--baseline
Prompt: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/prompts/code-feature-mcp-preflight-stable-registry--baseline.md

## Baseline Isolation
No Layers preflight command was executed.

## Agent Execution
Agent command: docs/dogfood/20260518T214500Z-phase5-30paired-preregistered-benchmark/scripts/gemini-agent-wrapper.sh
Agent exit status: 0
Agent stdout:
I will start by searching for `preflight_context` and `registry` within the `crates/layers-mcp` directory to understand how they are currently implemented and exposed.
I will read `crates/layers-mcp/src/stable.rs` to examine the implementation of `PreflightContextTool` and how it interacts with the compiler.
I will check the definition of `STABLE_CONTEXT_SURFACE_TOOLS` in `crates/layers-mcp/src/server.rs`.
I will read `crates/layers-compiler/src/lib.rs` to understand the functionality of `ContextCompiler`.
I will read `crates/layers-mcp/Cargo.toml` to check the dependencies.
I will read `crates/layers-core/src/lib.rs` to check the `ContextPacket` and `Provenance` structures.
I will read the first 300 lines of `crates/layers-core/src/lib.rs` to find the `ContextPacket` and `Provenance` structures.
I will read `crates/layers-core/src/context_packet.rs` to examine the `ContextPacket` and `Provenance` structures.
I will read the definition of `PacketProvenance` in `crates/layers-core/src/context_packet.rs`.
I will read the `compile` method in `crates/layers-compiler/src/lib.rs`.
I will add the regression guard tests to `crates/layers-mcp/src/stable.rs` to ensure `preflight_context` remains exposed and uses the compiler-backed path.
I will run the validation commands, starting with the tests for `layers-mcp`.
I will now run `cargo check --workspace --all-targets` to ensure everything is correct across the workspace.
I will check `crates/layers-mcp/src/lib.rs` to see if `stable_context_registry()` is used and where.
I will read `src/main.rs` to see how the MCP server is initialized.
I will read `src/cmd/mcp.rs` to see how MCP commands are handled.
I will read `crates/layers-mcp/src/server.rs` to see if `make_stable_registry` is used in production.
I will read the context around `make_stable_registry` in `crates/layers-mcp/src/server.rs`.
I will run all tests for `layers-mcp` again to ensure everything is correct.
I will run the tests for `layers-compiler` to ensure no regressions were introduced.
I will run the tests for `layers-core`.
I have added a regression guard to ensure that the stable MCP `preflight_context` remains exposed in the stable registry and routes packet generation through the compiler-backed path.

Specifically, I added two new tests to `crates/layers-mcp/src/stable.rs`:
1. `regression_guard_preflight_context_is_exposed_in_stable_registry`: Verifies that `preflight_context` is present in the `stable_context_registry` and is recognized as a stable context tool.
2. `regression_guard_preflight_context_uses_compiler_path`: Executes the tool and validates that the resulting `ContextPacket` has the correct provenance metadata (`surface: "preflight"`, `compiler: "layers-context-packet"`) and route label (`"preflight_context"`), ensuring it was processed by the `ContextCompiler`.

These tests serve as a focused regression suite for the requested behavior. I also verified that all existing tests and workspace checks pass.

I have implemented the requested regression guard in `crates/layers-mcp/src/stable.rs`. The new tests ensure that `preflight_context` is always exposed in the stable registry and that it uses the `ContextCompiler` for packet generation with the correct `Preflight` mode and `preflight_context` routing label.

Summary of changes:
- Added `regression_guard_preflight_context_is_exposed_in_stable_registry` to verify tool exposure.
- Added `regression_guard_preflight_context_uses_compiler_path` to verify the compiler-backed routing and metadata.
- Verified all changes with `cargo test -p layers-mcp` and `cargo check --workspace --all-targets`.

Agent stderr:
YOLO mode is enabled. All tool calls will be automatically approved.
YOLO mode is enabled. All tool calls will be automatically approved.


## Validation
Log: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/validation/code-feature-mcp-preflight-stable-registry--baseline.log

## Scoring Notes
Smoke execution record generated automatically; not product-effectiveness evidence.

## Context Quality Classification
Variant-scoped smoke classification; independent scoring required for claims.
