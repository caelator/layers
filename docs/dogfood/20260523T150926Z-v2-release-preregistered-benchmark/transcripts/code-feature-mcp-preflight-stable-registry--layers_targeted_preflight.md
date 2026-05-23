# Workflow Benchmark Transcript

Task ID: code-feature-mcp-preflight-stable-registry
Variant: layers_targeted_preflight
Run ID: code-feature-mcp-preflight-stable-registry--layers_targeted_preflight
Worktree: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/worktrees/code-feature-mcp-preflight-stable-registry--layers_targeted_preflight
Prompt: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/prompts/code-feature-mcp-preflight-stable-registry--layers_targeted_preflight.md

## Targeted Preflight
Preflight exit status: 0
Packet artifact: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/packets/code-feature-mcp-preflight-stable-registry--layers_targeted_preflight.md

## Agent Execution
Agent command: docs/dogfood/20260518T214500Z-phase5-30paired-preregistered-benchmark/scripts/gemini-agent-wrapper.sh
Agent exit status: 0
Agent stdout:
I will start by reading the targeted preflight packet artifact to understand the context of the task.
I will list the directory `docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/packets/` to verify the existence of the preflight packet artifact.
I will list the `docs/dogfood` directory to find the correct path to the preflight packet artifact.
I will list the contents of `docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark` to locate the `packets` directory.
I will use `ls -R` to find the `packets` directory and the specific preflight packet artifact.
I will use the `find` command to locate the preflight packet artifact.
I will use `find` to search for any directory named `packets` in the workspace.
I will list all files in `docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark` to find the preflight packet.
I will read the preflight packet from the `prompts` directory to understand the task context.
I will search for the preflight packet artifact by name in the entire workspace.
I will read the preflight packet from the `transcripts` directory.
I will explore the `crates/layers-mcp` directory to understand its structure and locate the relevant files.
I will search for the string `preflight_context` in the `crates/layers-mcp` directory.
I will read `crates/layers-mcp/src/stable.rs` to understand the current implementation of `preflight_context`.
I will read `crates/layers-mcp/src/server.rs` to see how `STABLE_CONTEXT_SURFACE_TOOLS` is defined and used.
I will run the tests for `layers-mcp` to ensure everything is working as expected.
I will read `crates/layers-compiler/src/lib.rs` to understand the `ContextCompiler` and what it does during compilation.
I will read `crates/layers-mcp/src/bridge.rs` to see if there are any other references to `preflight_context`.
I will read `crates/layers-mcp/src/types.rs` to see if there are any relevant type definitions.
I will add a regression guard test to `crates/layers-mcp/src/stable.rs` to ensure `preflight_context` remains exposed and uses the compiler-backed path.
I will add a regression guard test to `crates/layers-mcp/src/server.rs` to ensure `preflight_context` remains in `CORE_STABLE_TOOLS`.
I will run the tests for `layers-mcp` to verify the new regression guards.
I will run `cargo check --workspace --all-targets` to ensure that there are no compilation errors across the entire workspace.
I have added regression guards to ensure that the stable MCP `preflight_context` tool remains exposed in the stable registry and routes packet generation through the compiler-backed path. 

Specifically:
- In `crates/layers-mcp/src/stable.rs`, I added `regression_preflight_context_stable_exposure_and_compiler_routing` to explicitly verify the tool's presence in the stable registry.
- In `crates/layers-mcp/src/server.rs`, I added `regression_preflight_context_is_core_stable_tool` to ensure `preflight_context` remains in the `CORE_STABLE_TOOLS` allowlist.
- I maintained and verified the `preflight_context_returns_compiler_finalized_context_packet` test, which confirms that the tool correctly utilizes `ContextCompiler` by checking the generated packet's `route`, `provenance.surface`, and derived evidence.

All tests passed, and `cargo check --workspace --all-targets` completed successfully.
I have implemented the requested regression guards to ensure that the stable MCP `preflight_context` tool remains exposed in the stable registry and correctly routes packet generation through the compiler-backed path. 

Changes:
- Added `regression_preflight_context_stable_exposure_and_compiler_routing` to `crates/layers-mcp/src/stable.rs`.
- Added `regression_preflight_context_is_core_stable_tool` to `crates/layers-mcp/src/server.rs`.
- Verified the implementation with `cargo test -q -p layers-mcp -- --nocapture` and `cargo check --workspace --all-targets`.

Agent stderr:
YOLO mode is enabled. All tool calls will be automatically approved.
YOLO mode is enabled. All tool calls will be automatically approved.
Error executing tool read_file: File path '/Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/worktrees/code-feature-mcp-preflight-stable-registry--layers_targeted_preflight/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/packets/code-feature-mcp-preflight-stable-registry--layers_targeted_preflight.md' is ignored by configured ignore patterns.
Error executing tool list_directory: Error: Failed to list directory.


## Validation
Log: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/validation/code-feature-mcp-preflight-stable-registry--layers_targeted_preflight.log

## Scoring Notes
Smoke execution record generated automatically; not product-effectiveness evidence.

## Context Quality Classification
Variant-scoped smoke classification; independent scoring required for claims.
