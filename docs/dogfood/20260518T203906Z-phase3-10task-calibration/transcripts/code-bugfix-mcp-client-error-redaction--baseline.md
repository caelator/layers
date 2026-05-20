# Workflow Benchmark Transcript

Task ID: code-bugfix-mcp-client-error-redaction
Variant: baseline
Run ID: code-bugfix-mcp-client-error-redaction--baseline
Worktree: /Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/worktrees/code-bugfix-mcp-client-error-redaction--baseline
Prompt: /Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/prompts/code-bugfix-mcp-client-error-redaction--baseline.md

## Baseline Isolation
No Layers preflight command was executed.

## Agent Execution
Agent command: /Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/scripts/gemini-agent-wrapper.sh
Agent exit status: 0
Agent stdout:
I will begin by exploring the `crates/layers-mcp` directory to understand the current implementation of MCP client/server error formatting.
I will search for "Error", "Display", "Debug", "redact", and "secret" in `crates/layers-mcp/src` to identify where error formatting and sensitive data handling occurs.
I will examine `crates/layers-mcp/src/client.rs` and `crates/layers-mcp/src/server.rs` to identify where request payloads might be formatted into errors, specifically looking at `call_tool` and `handle_tool_call`.
I've examined `client.rs` and noted that `send_request` handles errors by echoing the server's error message or a serialization error, neither of which seems to directly echo the request payload. I will now examine `server.rs` to see how it handles request payloads and error formatting.
In `server.rs`, I've identified that `e.to_string()` in `handle_tools_call` could potentially echo sensitive information if the error contains arguments. I also noticed that `JsonRpcRequest` uses `Debug`, which could leak secrets if logged. I'll now examine `crates/layers-mcp/src/types.rs` for shared types that might need redaction, and I'm considering implementing a recursive redaction function for "secret-like" fields such as `api_key`, `token`, and `password`.
I've reviewed `types.rs` and am considering implementing a `redact` method for `serde_json::Value`. I'll check `crates/layers-core/src/lib.rs` to determine if `layers-core` is a suitable location for this logic, given that `crates/layers-mcp` lacks a `util.rs`.
Checking `crates/layers-core/src/types.rs`.
I've noticed that `AuthProfile` in `layers-core/src/types.rs` contains an `api_key`, which could be leaked in MCP error messages if included in `e.to_string()`. I've also identified that `serde_json::from_str` might leak sensitive information from malformed requests. I'll now create a reproduction test in `crates/layers-mcp/tests/error_redaction.rs` to verify if secrets are leaked when a tool call fails or a request is malformed.
I've identified potential secret leaks in `server.rs` and `client.rs`, particularly in error logging and JSON-RPC responses when requests are malformed or tools return error messages containing secrets. While `serde_json` errors typically don't echo data, I'm concerned about cases where request parameters might be explicitly formatted into errors. I'll now search for `format!` in both files to see if any request fields are being echoed.
I've reviewed `server.rs` and `client.rs` and found that while they report errors, they don't currently seem to echo tool call arguments. However, to comply with the mandate of never echoing secret-like fields, I'm considering implementing a generic redaction utility for `serde_json::Value` to be used in logging and error messages. I'll now examine `bridge.rs` for similar patterns.
I've identified a strategy to prevent sensitive information leaks in MCP error messages: I'll implement a `redact_secrets_with_context` utility that uses request arguments to identify and redact secrets from error strings. I'll integrate this into `server.rs`'s `handle_tools_call` and `client.rs`'s `send_request`, ensuring both sides of the communication are protected. I'll now implement these utilities in `crates/layers-mcp/src/types.rs`, covering common sensitive keywords like "key", "token", and "password".
I'll now update `handle_tools_call` in `server.rs` and then `client.rs` to incorporate the new redaction logic.
I'm updating the error handling in `client.rs`, specifically within the `send_request` function, which takes an optional `serde_json::Value` for `params`.
Checking `stable.rs` for error formatting.
`stable.rs` uses `serde_json::from_value(args)` which could potentially echo secrets if it fails, even though the defined parameters don't seem to contain sensitive fields. I've already addressed the primary entry points in `server.rs` and `client.rs`, but I'll consider applying `redact_secrets_with_context` in `stable.rs` as well to be thorough. I'll now proceed to verify these changes with tests.
Borrow checker errors; I need to clone the values.
Updating `server.rs`.
Retesting.
Tests passed, but `error_redaction.rs` was incomplete and only tested compilation. I'll now complete it by making `McpServer::handle_request` `pub(crate)` in `server.rs` to verify redaction in a real scenario.
I'll make `JsonRpcRequest` and `JsonRpcResponse` in `server.rs` `pub(crate)` since they're currently internal.
I'll move the redaction test from `tests/error_redaction.rs` to the end of the `tests` module in `crates/layers-mcp/src/server.rs` since integration tests can't access `pub(crate)` members.
Deleting the failed integration test.
I'm adding a redaction test to `crates/layers-mcp/src/client.rs`, modeled after the existing `json_rpc_error_response_framing` test. To test `send_request`, I'll either use a mock server or mock stdin/stdout. Checking `client.rs`'s tests now.
`send_request` in `McpClient` is private and hard to test due to its stdin/stdout interaction. While I've tested the redaction logic in `types.rs`, the prompt requires verification that secret-like values are redacted in errors. I've noted a potential leak in `server.rs` during JSON-RPC request parsing errors. Since parsing hasn't occurred yet, I can't know the secrets, so I'll consider using a heuristic to redact common secret patterns from error logs. I'll also refine `is_sensitive_key` and add a "Parse error" test case to `server.rs`.
I've addressed the primary concerns of the task, but I'll now check for potential secret leaks in `Debug` implementations and `format!` calls. While `serde_json::Error`'s `Display` typically doesn't echo the entire payload, I'll apply redaction in `stable.rs` where `from_value` fails on the request payload, just to be safe.
I'll apply redaction to `PreflightContextTool`, `ImpactAnalyzeTool`, and `ValidateContextTool` in `stable.rs` as well.
Searching for `serde_json::from_value(args)`.
Reading file.
Updating `stable.rs` tools.
Failed multi-replacement; I'll do them individually.
`ImpactAnalyzeTool` is next.
`ValidateContextTool` is the last one.
Rerunning all tests, `test_server_error_redaction` included.
Tests passed. Now, `cargo check --workspace --all-targets` to ensure workspace integrity.
`cargo check` passed. I've implemented redaction utilities in `types.rs`, integrated them into `server.rs`, `client.rs`, and `stable.rs`, and added unit and integration tests. I'm keeping the `pub(crate)` changes in `server.rs` to support the new `test_server_error_redaction` test. Now, I'll add one final test in `types.rs` to more closely simulate the client/server interaction.
Final test run.
Brace error in `types.rs`.
`replace` tool brace error; fixing now.
Retesting.
`test_redact_various_types` failed because `42` is too short to be redacted by my `secret.len() >= 3` rule, which exists to prevent over-redaction. I'll update the test to use `424242` instead.
Rerunning tests.
Redaction implemented and verified. Final `cargo check --workspace --all-targets` commencing.
Submitting.
I have ensured that MCP client and server error formatting never echoes secret-like request payload fields by implementing a robust redaction mechanism.

### Summary of Changes

1.  **Redaction Utilities**: Added `redact_secrets_with_context`, `collect_secrets`, and `is_sensitive_key` to `crates/layers-mcp/src/types.rs`. These utilities recursively identify sensitive fields (e.g., `api_key`, `token`, `secret`) in a JSON payload and redact their values from any associated error message strings.
2.  **Server-Side Protection**: Updated `McpServer::handle_tools_call` in `crates/layers-mcp/src/server.rs` to redact secrets from error messages returned by tool executions.
3.  **Client-Side Protection**: Updated `McpClient::send_request` in `crates/layers-mcp/src/client.rs` to redact secrets from errors returned by the MCP server before they are propagated into `LayersError`.
4.  **Stable Tool Protection**: Applied redaction to all stable product-facing tools in `crates/layers-mcp/src/stable.rs` (including `ContextCompileTool`, `PreflightContextTool`, etc.) to protect against leaks during parameter parsing.
5.  **Comprehensive Testing**: 
    - Added unit tests for the redaction logic in `types.rs`, covering various data types and edge cases.
    - Added an integration-style test in `server.rs` (`test_server_error_redaction`) that simulates a tool leaking an API key and verifies it is correctly redacted in the final MCP response.

### Verification Results

- `cargo test -q -p layers-mcp -- --nocapture`: **All 44 tests passed.**
- `cargo check --workspace --all-targets`: **Passed.**

These changes ensure that even if a tool or a remote server inadvertently includes sensitive request data in an error message, it will be intercepted and redacted before being exposed to the user or logged.

Agent stderr:
YOLO mode is enabled. All tool calls will be automatically approved.
YOLO mode is enabled. All tool calls will be automatically approved.
Error executing tool replace: Error: Failed to edit, could not find the string to replace.


## Validation
Log: /Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/validation/code-bugfix-mcp-client-error-redaction--baseline.log

## Scoring Notes
Smoke execution record generated automatically; not product-effectiveness evidence.

## Context Quality Classification
Variant-scoped smoke classification; independent scoring required for claims.
