## Problem Brief
- The existing route-correction feedback infrastructure in `src/feedback.rs` is currently disconnected from the active execution path, preventing failure data from influencing future routing decisions.
- "Done" entails a fully wired system where hard errors (Timeouts, NonZeroExits) and soft failures (low-quality or empty query results) automatically trigger `emit_failure()` or `RouteFailure::Soft` respectively.
- The system must successfully read `route-corrections.jsonl` at startup to adjust internal routing weights, and the `Router` must actively consult these historical failure weights during active execution.

## Constraints
- The existing test suite must remain completely green (117/117 tests passing).
- Code must pass `cargo fmt --all` and `cargo clippy` with zero warnings.
- The serialization format for the `RouteFailure` enum must maintain strict backward compatibility with existing data logs.
- All newly introduced public APIs must include comprehensive Rustdoc comments.

## Success Criteria
- Simulating a `Timeout` or `NonZeroExit` during council execution demonstrably writes a new hard failure entry to the feedback loop log.
- Receiving an empty or low-quality query result demonstrably records a `RouteFailure::Soft` event.
- Upon application startup, the system successfully parses `route-corrections.jsonl` without errors and populates the in-memory failure history.
- The `Router` actively alters its routing path or selection weights when presented with a route that has a documented history of failures in the loaded corrections data.

## Out of Scope
- Developing new types of feedback signals or entirely new error categorizations beyond the existing hard and soft failures.
- Implementing a remote synchronization mechanism, API, or distributed database for sharing `route-corrections.jsonl` across multiple instances or environments.
- Building a UI, CLI visualization tool, or dashboard for rendering the route correction weights and historical failure data.
