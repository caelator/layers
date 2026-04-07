## 1. Goal and constraints
- Wire existing route-correction feedback infrastructure (`src/feedback.rs`) into active execution paths.
- Convert Timeout and NonZeroExit occurrences in council execution into Hard `emit_failure()` calls.
- Convert low-quality or empty query results in `src/cmd/query.rs` into Soft `RouteFailure` records.
- Load `route-corrections.jsonl` dynamically on startup and apply historical failure weights inside the Router (`src/router.rs`).
- Constraints: Maintain completely green test suite (117/117 passing tests), enforce `cargo fmt --all` and `cargo clippy` with zero warnings, keep `RouteFailure` JSON serialization strictly backward compatible, and include comprehensive Rustdocs for all new public APIs.

## 2. Candidate approaches
- **Approach A: Inline Synchronous Emission & Lazy Caching**
  - Inject `emit_failure` directly into the `council::stage::run` execution loop (where process status evaluates to Timeout or NonZeroExit) and inside `cmd::query::run` for empty results.
  - Rely on `router::build_correction_cache()` (lazy initialization) to parse `route-corrections.jsonl` into a static mapping, applying score penalties directly inside the `classify()` function.
  - *Tradeoffs:* Conceptually simple, minimal footprint, avoids concurrency overhead, fits monolithic design perfectly. The only downside is synchronous file I/O blocking execution thread for a fraction of a millisecond.
- **Approach B: Channel-Based Async Feedback & Global Context**
  - Run a background MPSC listener thread that ingests raw failure objects, performs batched asynchronous writes to the JSONL, and atomically updates an `Arc<RwLock<...>>` memory cache.
  - *Tradeoffs:* Non-blocking for execution threads, but introduces high concurrency complexity, potentially flaking existing tests with timing issues, and is over-engineered for a local CLI tool doing single-session tasks.

## 3. Recommended approach with rationale
- **Approach A (Inline Synchronous & Lazy Caching)** is recommended.
- *Rationale:* Layers operates primarily as a local CLI tool. Synchronously appending a single line to a JSONL file via `StorageSafety::atomic_write` takes less than a millisecond. Keeping the logic synchronous strictly limits async runtime overhead, entirely avoids test flakiness related to channels, and ensures the 117/117 test constraint can be confidently met without architectural sweeping changes.

## 4. V1 scope
- **Hard Failures (`src/council/stage.rs`):** Intercept the matching logic inside `stage.rs` where `status` returns `None` (Timeout) or `Some(exit)` where `exit.success()` is false (NonZeroExit), and synchronously invoke `emit_failure(RouteFailure::new(HardErrorKind::...))`.
- **Soft Failures (`src/cmd/query.rs`):** In `run_query`, detect when the retrieved result lines are empty or confidence is critically low, and call `emit_failure` with `SoftErrorKind::InsufficientContext` or `EmptyResult`.
- **Router Integration (`src/router.rs`):**
  - Modify `build_correction_cache()` to robustly read and parse `~/.layers/route-corrections.jsonl` into the cache map on first router usage.
  - Modify `classify(task: &str)` to check the parsed historical failure data. If a route matching the task heuristics has a high historical error count, dynamically penalize its `structural`/`historical` scores or force it to `Confidence::Low`.

## 5. V2 / later
- Distributing or synchronizing `route-corrections.jsonl` across multiple workstations or shared developer environments.
- Building an interactive CLI dashboard (`layers feedback list` or `layers graph`) to visualize, monitor, and selectively prune historical route weights.
- Time-based decay of corrections: aging out old records automatically so long-resolved bugs don't indefinitely penalize valid routing paths.

## 6. Out of scope / do not build
- Designing entirely new classes of `RouteFailure` beyond what is currently established in `src/feedback.rs`.
- Replacing the simple JSONL flat-file schema with a relational database like SQLite.
- Creating remote APIs to broadcast route failures to a centralized server.

## 7. Files, binaries, and storage
- `src/council/stage.rs` — Modifying the process execution loop to detect `Timeout` and `NonZeroExit` and invoke `emit_failure()`.
- `src/cmd/query.rs` — Modifying the query execution boundary to capture soft failures like empty retrievals.
- `src/router.rs` — Enhancing cache loading and incorporating the penalty logic into the `classify` heuristic model.
- `src/feedback.rs` — Relying on its existing types and functions (no major logic refactors, but ensuring APIs are fully documented).
- `~/.layers/route-corrections.jsonl` — The target persistent storage file for reading route adjustments and writing feedback emissions.

## 8. Control flow
1.  **Startup:** A user issues a Layers command that requires routing.
2.  **Cache Load:** `router::classify()` lazily invokes `build_correction_cache()`, reading all valid JSON lines from `~/.layers/route-corrections.jsonl` into memory.
3.  **Routing Phase:** The router calculates its heuristic scores based on text signals, checks the memory cache, and penalizes the scores of routes that have a history of failing for this context.
4.  **Execution & Hard Failure:** If a council stage executes a subprocess and hits a hard boundary (e.g., `try_wait` hits timeout threshold, or exit code != 0), a `RouteFailure` (`HardErrorKind`) is constructed using the current task context and synchronously appended to the JSONL.
5.  **Execution & Soft Failure:** If a `query` operation returns zero lines, a `RouteFailure` (`SoftErrorKind::InsufficientContext`) is appended.
6.  **Next Run:** The feedback loop is closed; subsequent runs parsing the JSONL will inherently avoid the failing path.

## 9. Risks and open questions
- *File I/O overhead:* Does reading `route-corrections.jsonl` block startup too long if the file grows to tens of thousands of lines? (Mitigation: Local file parsing is fast, but V2 might need a truncation/decay strategy).
- *Routing Cycle Context:* Does `src/council/stage.rs` have access to the original `task` string and the `predicted`/`actual` `RouteId` needed to construct a complete `RouteFailure` payload? If not, `CouncilStage` or `CouncilRun` may need to pass down the routing context when spinning up the subprocess.

## 10. Validation plan
- Write a unit test simulating an artificial subprocess `Timeout` directly in `council/stage.rs` and verify `~/.layers/route-corrections.jsonl` contains the exact expected JSON entry.
- Write a unit test triggering a `NonZeroExit` (`exit 1`) in a stage and verify the correct failure record is written.
- Write a unit test in `cmd/query.rs` passing an empty result and ensure a `SoftErrorKind` is recorded.
- Provide a populated `route-corrections.jsonl` fixture where `Route::Both` is highly penalized. Instantiate the engine and assert `classify()` correctly demotes its confidence/selection despite keyword matching.
- Run `cargo fmt --all && cargo clippy` ensuring 0 warnings.
- Run `cargo test` confirming exactly 117/117 pass.
