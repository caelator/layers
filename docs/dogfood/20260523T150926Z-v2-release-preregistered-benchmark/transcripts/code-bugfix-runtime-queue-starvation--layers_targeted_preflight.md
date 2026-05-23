# Workflow Benchmark Transcript

Task ID: code-bugfix-runtime-queue-starvation
Variant: layers_targeted_preflight
Run ID: code-bugfix-runtime-queue-starvation--layers_targeted_preflight
Worktree: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/worktrees/code-bugfix-runtime-queue-starvation--layers_targeted_preflight
Prompt: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/prompts/code-bugfix-runtime-queue-starvation--layers_targeted_preflight.md

## Targeted Preflight
Preflight exit status: 0
Packet artifact: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/packets/code-bugfix-runtime-queue-starvation--layers_targeted_preflight.md

## Agent Execution
Agent command: docs/dogfood/20260518T214500Z-phase5-30paired-preregistered-benchmark/scripts/gemini-agent-wrapper.sh
Agent exit status: 0
Agent stdout:
I will start by listing the files in the `crates/layers-runtime/src` directory to locate the relevant queue implementation files.
I will read the `queue.rs` file and the targeted preflight packet to understand the current implementation and the reported issue.
I will search for "critical" and "fairness" or "ratio" in the codebase to find where the fairness logic is implemented.
I will read the `src/critical_path.rs` file to see if it contains the fairness logic.
I will add a reproduction test case to `src/critical_path.rs` to demonstrate the starvation of standard tasks when the dispatcher is full.
I will refine the test case to be more aggressive and run it to check for standard task starvation.
I will write a test case to check the ratio of started tasks to see if it deviates from the 8:1 ratio.
I will run the test to see if standard tasks are getting their fair share.
I will write a test case to demonstrate the turn loss for standard tasks when the dispatcher is full.
I will run the test to see if standard tasks lose their turn.
I will search for usages of `dequeue` and `acquire` to see how they are used in the codebase.
I will read `src/council/mod.rs` around line 269 to see how `acquire` is used.
I will search for `critical_path` in `crates/layers-runtime`.
I will modify `src/critical_path.rs` to implement the fix, starting with updating `try_dequeue_inner` and adding `try_dequeue_fair`.
I will implement the changes to `WeightedFairQueue` and `Dispatcher` to fix the starvation issue.
I will read `src/critical_path.rs` again to ensure I have the correct content for the `replace` tool.
I will update the `WeightedFairQueue` methods in `src/critical_path.rs`.
I will update the `try_dequeue_inner` function in `src/critical_path.rs`.
I will update the `Dispatcher::acquire` method in `src/critical_path.rs`.
I will update the `test_standard_starvation_turn_loss` test case in `src/critical_path.rs` to reflect the fix and verify that standard tasks no longer lose their turn.
I will use a smaller part of the `old_string` to update the `test_standard_starvation_turn_loss` test case.
I will search for the `test_standard_starvation_turn_loss` function and replace its body.
I will update the `test_standard_starvation_turn_loss` test case using the correct content.
I will run all the tests in `src/critical_path.rs` to verify the fix.
I will fix the `test_standard_starvation_turn_loss` test case by correcting the expected item IDs.
I will run the tests in `src/critical_path.rs` again to verify the fix.
I will fix the `test_standard_starvation_turn_loss` test case by releasing the worker slots as needed.
I will run the tests in `src/critical_path.rs` again to verify the fix.
I will run the validation command `cargo test -q -p layers-runtime -- --nocapture` to ensure no regressions in the `layers-runtime` crate.
I will do a final check of `src/critical_path.rs` to ensure the code is clean and there are no lint errors.
I will search for any remaining `try_dequeue` calls to ensure they are all updated correctly.
I will run the tests in `src/critical_path.rs` one last time to ensure everything is correct.
I have diagnosed and fixed the runtime queue fairness regression where critical tasks could starve standard tasks.

The root cause was twofold:
1. `Dispatcher::acquire` would dequeue a task and, if it couldn't be run due to slot limits (e.g., standard slots full), it would re-enqueue it at the back of the queue. This broke FIFO order and caused tasks to lose their position.
2. `WeightedFairQueue::try_dequeue_inner` would advance the round-robin position even when a task was dequeued but couldn't be run, effectively "wasting" the turn for that priority and forcing it to wait for a full cycle (8 more critical tasks) before its next turn.

I implemented a "fair dequeue" mechanism:
- `Dispatcher::acquire` now checks worker slot availability *before* dequeuing.
- `WeightedFairQueue` now supports conditional dequeuing via `try_dequeue_fair`, which only pops an item if it can actually be started.
- The internal scheduling logic was updated to only advance the round-robin position when a task is successfully dequeued from its preferred lane, or when it falls through to a non-preferred lane because the preferred lane was empty (but NOT when it's blocked by worker limits).

I added two deterministic regression tests to `src/critical_path.rs`:
- `test_standard_starvation_ratio`: Verifies that the 8:1 ratio is maintained even under heavy load and saturation.
- `test_standard_starvation_turn_loss`: Specifically verifies that standard tasks do not lose their turn in the rotation when the dispatcher is full for standard tasks but not for critical tasks.

All validation commands passed successfully.
I have fixed the runtime queue fairness regression where critical tasks could starve standard tasks. I updated `WeightedFairQueue` and `Dispatcher` to implement a fair, conditional dequeue mechanism that avoids wasting turns and re-enqueuing tasks when worker slots are full. I also added deterministic regression tests and verified that all tests pass.
I've completed the task. The fairness regression in the runtime queue is fixed, and deterministic tests have been added to prevent future regressions.
I've fixed the runtime queue fairness regression where critical tasks could starve standard tasks beyond the configured 8:1 ratio. The solution involves implementing a conditional dequeue mechanism that avoids wasting turns and re-enqueuing tasks when worker slots are saturated. I also added deterministic regression tests to `src/critical_path.rs`.

Agent stderr:
YOLO mode is enabled. All tool calls will be automatically approved.
YOLO mode is enabled. All tool calls will be automatically approved.
Error executing tool read_file: File path '/Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/worktrees/code-bugfix-runtime-queue-starvation--layers_targeted_preflight/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/packets/code-bugfix-runtime-queue-starvation--layers_targeted_preflight.md' is ignored by configured ignore patterns.
Error executing tool replace: Error: Failed to edit, could not find the string to replace.
Error executing tool replace: Error: Failed to edit, could not find the string to replace.
Error executing tool replace: Error: Failed to edit, could not find the string to replace.


## Validation
Log: /Users/xxx/layers/docs/dogfood/20260523T150926Z-v2-release-preregistered-benchmark/validation/code-bugfix-runtime-queue-starvation--layers_targeted_preflight.log

## Scoring Notes
Smoke execution record generated automatically; not product-effectiveness evidence.

## Context Quality Classification
Variant-scoped smoke classification; independent scoring required for claims.
