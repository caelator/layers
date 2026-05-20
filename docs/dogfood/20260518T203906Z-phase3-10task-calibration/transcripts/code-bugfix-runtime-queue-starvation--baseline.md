# Workflow Benchmark Transcript

Task ID: code-bugfix-runtime-queue-starvation
Variant: baseline
Run ID: code-bugfix-runtime-queue-starvation--baseline
Worktree: /Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/worktrees/code-bugfix-runtime-queue-starvation--baseline
Prompt: /Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/prompts/code-bugfix-runtime-queue-starvation--baseline.md

## Baseline Isolation
No Layers preflight command was executed.

## Agent Execution
Agent command: /Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/scripts/gemini-agent-wrapper.sh
Agent exit status: 0
Agent stdout:
I will start by listing the files in `crates/layers-runtime` to get an overview of the crate's structure.
I will read the content of `crates/layers-runtime/src/queue.rs` to understand the current queue implementation and fairness logic.
I will search for the terms "critical", "priority", and "ratio" in the `crates/layers-runtime` directory to find where the fairness logic is implemented.
I will search for "ratio" and "fairness" across the entire repository and also locate the definition of `InboundMessage` to see if it includes priority information.
I will search for usages of `SessionQueue` in `crates/layers-runtime` to identify if another struct implements the fairness logic.
I will read `crates/layers-runtime/src/actor.rs` to see how it uses `SessionQueue`.
I'll read `crates/layers-runtime/src/lib.rs` to find other modules that might contain the fairness logic.
I'll check `crates/layers-runtime/src/engine.rs` and search for "critical" in `crates/layers-core`.
I'll search for "Fair" or "Queue" throughout the entire project to find any relevant fairness logic.
I'll read `src/critical_path.rs` to examine the weighted fair queueing implementation and the dequeue ratio logic.
I'll run the tests for `layers-runtime` and `critical_path` to see if I can identify any existing issues or failing tests.
I'll create a new test file `tests/queue_starvation.rs` to investigate the 8:1 ratio and identify any potential starvation issues.
I'll check the root `src/lib.rs` to see if `critical_path` is exported and how to access it.
I'll check the root `Cargo.toml` to confirm the crate name and its dependencies.
I'll update the `tests/queue_starvation.rs` file to correctly use `layers::critical_path` and ensure the integration test can access the library crate.
I'll run the newly created `tests/queue_starvation.rs` test to see if it reveals any starvation issues.
I'll write a new test in `tests/queue_starvation.rs` that uses `Dispatcher::acquire` with multiple threads to see if standard work is starved when the system is under load.
I'll run the `test_dispatcher_starvation` test. If it fails, it will support my theory that standard tasks are being starved because their turns are being wasted when re-enqueued.
I'll check `crates/layers-runtime/src/agent_loop.rs` for any other queueing logic that might be relevant.
I'll check `src/main.rs` to see how the dispatcher is used and if there are any clues about the starvation issue.
I'll search for `global_dispatcher()` in `src/main.rs` to see how it's being used.
I'll examine `src/council/mod.rs` to see how the global dispatcher is used.
I'll create a regression test in `tests/queue_starvation.rs` to demonstrate that the current implementation allows critical work to starve standard work beyond the 8:1 ratio, specifically targeting the turn-stealing and position-advancement logic.
I'll run the test.
I'll refactor `try_dequeue_inner` to implement the "no-advance-on-fallback" logic, ensuring that `round_position` is only incremented when an item is taken from its preferred lane, which should fix the starvation issue and satisfy the test.
I'll rerun the regression test.
I'll add the `test_ratio_under_heavy_load` back to `tests/queue_starvation.rs` to ensure the 8:1 ratio is still maintained under heavy load with my changes.
I'll run all the tests.
I'll check `src/critical_path.rs` to see if `push_front` is used or if I should implement it for re-enqueuing to prevent standard tasks from being starved.
I'll implement a `re_enqueue` method in `WeightedFairQueue` that uses `push_front`, and then update `Dispatcher::acquire` to use this new method, ensuring that re-enqueued tasks aren't pushed to the back of the queue.
I'll update `Dispatcher::acquire` to use the `re_enqueue` method.
I'll rerun the tests to see if the "no-advance-on-fallback" and "re-enqueue at front" changes have improved the results of `test_dispatcher_starvation`.
I'll add the `test_dispatcher_starvation` test back to `tests/queue_starvation.rs` to verify that my changes improved the fairness of the dispatcher.
I'll run the tests again.
I'll rerun the `layers-runtime` tests one last time to ensure everything is functioning correctly after the fix for standard work starvation.
I've diagnosed and fixed the starvation bug by modifying `try_dequeue_inner` to only advance `round_position` on preferred-lane dequeues and adding a `re_enqueue` method to `WeightedFairQueue`. I'll now move the regression test from `tests/queue_starvation.rs` to `src/critical_path.rs` and then delete the temporary test file.
I'll add the regression tests to `src/critical_path.rs`.
I'll remove the temporary test file.
I'll run all the tests one last time.
I've fixed the runtime queue fairness regression by ensuring `round_position` only advances on preferred-lane dequeues and adding `re_enqueue` at the front of the queue. I've also added deterministic regression tests to `src/critical_path.rs` and verified that all tests pass.
I've fixed the runtime queue fairness regression where critical work could starve standard work beyond the 8:1 ratio. The root cause was that `try_dequeue_inner` advanced the round-robin position even when a lane "stole" a turn from an empty lane, causing standard items that arrived just after their slot was stolen to wait for a full cycle of 8 critical items. I modified the logic to only advance the position on preferred-lane dequeues and implemented `re_enqueue` at the front of the queue to improve fairness. Deterministic regression tests were added to `src/critical_path.rs`.
I have successfully diagnosed and fixed the runtime queue fairness regression.

### Key Changes:
- **Fairness Logic Fix:** Modified `WeightedFairQueue::try_dequeue_inner` to only advance the round-robin position when an item is dequeued from its preferred lane. This prevents critical work from "stealing" standard work slots and forcing standard items to wait for a full cycle when they arrive just after a stolen turn.
- **Re-enqueue Optimization:** Added a `re_enqueue` method to `WeightedFairQueue` that inserts items at the front of their respective lanes. Updated `Dispatcher::acquire` to use this method when a task cannot be started due to worker slot limitations, ensuring that high-waiting tasks aren't penalized further by being moved to the back of the queue.
- **Regression Tests:** Added `no_starvation_beyond_ratio_when_stealing` and `re_enqueue_at_front_preserves_order` to the `critical_path` test suite to verify the fix and prevent future regressions.

### Validation:
- All `layers-runtime` and `critical_path` tests passed.
- Deterministic regression test confirmed that standard items are now served immediately when their turn is available, even if a turn was previously stolen while the queue was empty.

```bash
cargo test -q critical_path -- --nocapture
cargo test -q -p layers-runtime -- --nocapture
```
Both commands passed successfully.

Agent stderr:
YOLO mode is enabled. All tool calls will be automatically approved.
YOLO mode is enabled. All tool calls will be automatically approved.


## Validation
Log: /Users/xxx/layers/docs/dogfood/20260518T203906Z-phase3-10task-calibration/validation/code-bugfix-runtime-queue-starvation--baseline.log

## Scoring Notes
Smoke execution record generated automatically; not product-effectiveness evidence.

## Context Quality Classification
Variant-scoped smoke classification; independent scoring required for claims.
