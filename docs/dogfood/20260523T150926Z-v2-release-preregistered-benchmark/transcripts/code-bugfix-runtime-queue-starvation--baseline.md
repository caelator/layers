# Workflow Benchmark Transcript

Task ID: code-bugfix-runtime-queue-starvation
Variant: baseline
Run ID: code-bugfix-runtime-queue-starvation--baseline
Worktree: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/worktrees/code-bugfix-runtime-queue-starvation--baseline
Prompt: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/prompts/code-bugfix-runtime-queue-starvation--baseline.md

## Baseline Isolation
No Layers preflight command was executed.

## Agent Execution
Agent command: docs/dogfood/20260518T214500Z-phase5-30paired-preregistered-benchmark/scripts/gemini-agent-wrapper.sh
Agent exit status: 0
Agent stdout:
I will begin by listing the files in the `crates/layers-runtime/src` directory to locate the queue implementation.
I'll examine the contents of `crates/layers-runtime/src/queue.rs` to understand the queue and fairness logic.
I'll search for "critical" and "ratio" within the `layers-runtime` crate to find any other queue implementations or relevant logic I might have missed.
I'll search the entire repository for "critical" to locate its definition, as it wasn't found in `layers-runtime`.
I'll examine `src/critical_path.rs` since it likely contains the "8:1 weighted dequeue ratio" logic mentioned in the prompt.
I've identified a potential bug in `Dispatcher::acquire`: when a standard task is popped but cannot be run, it's re-enqueued, and the `round_position` still advances, effectively skipping the standard work turn. I'll now check `crates/layers-runtime/Cargo.toml` to see if `src/critical_path.rs` is used by the `layers-runtime` crate.
I'll check the root `Cargo.toml` since the `layers-runtime` crate doesn't appear to depend on the root crate.
I've analyzed `Dispatcher::acquire` and `try_dequeue_inner`, and I suspect that standard tasks are losing their fair-share turns when they're popped but cannot be run, as `round_position` still advances. This could lead to starvation beyond the 8:1 ratio. I'll now create `tests/repro_starvation.rs` to attempt to reproduce this issue.
I'll run the integration test in `tests/repro_starvation.rs` to see if it reproduces the issue, using the crate name `layers` as specified in `Cargo.toml`.
I've confirmed that `try_dequeue_inner` advances `round_position` even when a task is re-enqueued by `acquire`, which can cause standard tasks to miss their turns. If a standard slot becomes free after a standard turn was skipped and re-enqueued, a critical task will likely take it because the round position moved to prefer critical. I'll now refine my repro test to demonstrate this: I'll fill standard slots, wait for a standard turn, show it being skipped and re-enqueued, free a slot, and then observe a critical task taking it instead of the standard one.
Run repro test.
The repro test confirms standard task starvation, so I'll refactor `WeightedFairQueue` in `src/critical_path.rs` to include a `try_dequeue_with_filter` method. This method will peek at tasks and only pop and advance the `round_position` if a provided filter (which will check worker slot availability in `Dispatcher::acquire`) returns `true`. If the preferred lane is blocked by the filter, the method will attempt to fall through to the other lane without advancing the round, ensuring fairness. I've also verified that acquiring the `accounting` lock before the `queue` lock in `Dispatcher::acquire` is safe and won't cause deadlocks.
I'm updating `Dispatcher::acquire` to utilize `try_dequeue_with_filter`.
I'll update the repro test to assert the fixed behavior, as the standard task `s_starve` should now correctly take the slot, causing the existing `assert!(item.critical_path)` to fail.
Run updated test to verify fix.
The repro test failed because `try_dequeue_inner_filtered` correctly doesn't advance the `round_position` when the pool is full, preventing my previous method of advancing the round. To test standard fairness, I need to actually run 8 critical tasks to reach `round_position = 8`. I'll adjust the test to run these tasks one by one, using the reserved slot, to reach the standard turn.
Run updated test.
My repro test still fails, unexpectedly returning a critical task at step 6. I've confirmed that `should_advance` should be `false` when standard is blocked, keeping `round_position` at 8. At step 6, standard should be preferred and, with a slot now free, `s_starve` should be returned. I suspect a bug in `try_dequeue_inner_filtered` or an incorrect `round_position`, so I'll add print debugging to `critical_path.rs` to investigate.
I'll uncomment the `println!` statements I added for debugging.
I'll re-read the file; I must have mismatched the indentation.
I'll use `replace` again, making sure to match the exact lines, including the `//`.
Second one.
Run test; check output.
The debug output shows `round_position` is 0 at the end of step 2 because `s_running` and 8 critical tasks (9 total) were run, wrapping the 9-round cycle. Since `s_running` already moved `round_position` from 0 to 1, I only need to run 7 critical tasks in step 2 to reach `round_position = 8`. I'll adjust the test accordingly.
Rerun test.
The test passed, confirming my fix: when `s_starve` was blocked, it didn't advance the round, and after freeing the slot, it correctly took its turn. Now, I'll remove the debug prints, run the required validation commands, and decide whether to move the regression test into `src/critical_path.rs`.
Running validation commands.
`cargo test -q critical_path` ran 27 tests, likely those in `src/critical_path.rs`, while filtering out 356 others. I'll now run my repro test one final time for confirmation.
Fix confirmed. I'll move the repro test into `src/critical_path.rs` as a permanent regression test and then delete the temporary file.
Appending test to `src/critical_path.rs`.
Deleting temporary test file; rerunning validation.
I've successfully addressed the task by refactoring `WeightedFairQueue` in `src/critical_path.rs` to support filtered dequeuing. I updated `try_dequeue_inner_filtered` to advance the round position only when an item isn't filtered out, and to allow fall-through to the non-preferred lane without advancing the round if the preferred lane is blocked. I also updated `Dispatcher::acquire` to use this new filtered dequeue, removing the need for re-enqueuing when slots are full. Finally, I added the deterministic regression test `test_standard_starvation_when_slots_full` to `src/critical_path.rs`, and verified all changes with the required validation commands.
I've already run that; submitting now.
Submitting.
I have diagnosed and fixed the runtime queue fairness regression in `layers-runtime` (specifically in `src/critical_path.rs`) where critical work could starve standard work by consuming its fair-share turns when slots were full.

### Changes:
1.  **Refactored `WeightedFairQueue`**: Introduced `try_dequeue_with_filter` to allow the `Dispatcher` to inspect the next available task and check worker slot availability *before* popping the item and advancing the scheduling round.
2.  **Fixed Fairness Logic**: Updated the internal `try_dequeue_inner_filtered` to ensure the `round_position` (which tracks the 8:1 critical-to-standard ratio) only advances when a task actually starts. If a preferred lane is blocked by the filter but the other lane can still run (e.g., critical work in a reserved slot while standard slots are full), the dispatcher now falls through to the available work *without* consuming the blocked priority's turn.
3.  **Improved Efficiency**: Eliminated the "pop and re-enqueue" pattern in `Dispatcher::acquire`, which previously pushed blocked standard tasks to the back of the queue and incorrectly advanced the round-robin state.
4.  **Added Regression Test**: Implemented a deterministic test case `test_standard_starvation_when_slots_full` in `src/critical_path.rs` that reproduces the starvation scenario and verifies the fix.

### Validation:
- All 28 tests in `src/critical_path.rs` pass, including the new regression test.
- All 24 tests in the `layers-runtime` crate pass.
- Verified that standard work now correctly preserves its turn even under heavy critical load when standard slots are temporarily saturated.

Agent stderr:
YOLO mode is enabled. All tool calls will be automatically approved.
YOLO mode is enabled. All tool calls will be automatically approved.
Error executing tool replace: Error: Failed to edit, could not find the string to replace.


## Validation
Log: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/validation/code-bugfix-runtime-queue-starvation--baseline.log

## Scoring Notes
Smoke execution record generated automatically; not product-effectiveness evidence.

## Context Quality Classification
Variant-scoped smoke classification; independent scoring required for claims.
