# Phase III Mid-Phase Completion Plan

## Objective

Close Phase III honestly from the current state: a real council workflow exists, tests/validate are green, and durable run artifacts are written, but the phase is not yet complete enough to claim reliable council closure inside Layers.

This plan is intentionally narrow. It only covers what is still required for:

- council execution inside Layers
- Memoryport continuity through curated records
- GitNexus-backed code understanding during council runs

It does not reopen architecture, create a general multi-agent platform, or expand Layers into broad workflow orchestration.

## Current Mid-Phase Reality

Already landed:

- fixed Gemini -> Claude -> Codex council execution
- durable run artifact directory with prompts, stdout/stderr, `run.json`, and `convergence.json`
- per-stage retry/timeout handling
- trace writeback into `memoryport/council-traces.jsonl`
- green tests for happy path and transient retry
- `validate` visibility into council command configuration

Not yet good enough to call complete:

- artifact correctness is only partially enforced
- convergence is inferred by weak output heuristics
- liveness coverage does not yet prove timeout, stall, and terminal-failure behavior well enough
- grounding is present but not yet validated as a contract
- no explicit promotion path from council outcomes into canonical curated memory
- operator-facing completion/status semantics are still too soft to support an honest “Phase III complete” claim

## Remaining Implementation Tasks

### 1. Tighten artifact contracts

Required work:

- Define the required artifact set for every completed council run:
  - `context.txt`
  - `context.json`
  - `run.json`
  - `convergence.json`
  - `gemini-prompt.txt`
  - `claude-prompt.txt`
  - `codex-prompt.txt`
  - at least one stdout/stderr pair per attempted stage
- Ensure `run.json` and `convergence.json` always agree on terminal state.
- Ensure failed or incomplete runs still persist enough state to diagnose what happened.
- Add explicit validation of artifact paths recorded inside `run.json`.

Completion intent:

- a council run should leave behind an inspectable, self-consistent record whether it succeeds, retries, times out, or fails

### 2. Strengthen convergence recording

Required work:

- Replace or harden the current string-match convergence detection so completion is not dependent on one free-text marker alone.
- Define a minimal Codex-stage response contract that makes these fields extractable and testable:
  - decision summary
  - unresolved risks
  - next steps
  - convergence state
- Make non-converged outcomes explicit rather than silently “recorded”.

Completion intent:

- convergence must be a trustworthy run outcome, not a formatting accident

### 3. Add explicit curated-memory promotion for durable outcomes

Required work:

- Add the smallest explicit path that can promote a completed council result into canonical curated memory records when warranted.
- Limit promoted record types to:
  - decision
  - constraint
  - status
  - next step
- Keep promotion explicit and reviewable; do not infer/promote silently from every trace.

Completion intent:

- durable conclusions from councils can become first-class Memoryport continuity, instead of remaining trapped in trace text only

### 4. Improve operator honesty and terminal reporting

Required work:

- Make council terminal states operator-clear:
  - `completed`
  - `incomplete`
  - `failed`
- Make the CLI/reporting output explain why a run is incomplete or failed:
  - timed out
  - empty output
  - retries exhausted
  - convergence not reached
- Ensure `validate` reports only configuration/readiness facts, not implied proof that council execution is complete.

Completion intent:

- the operator must be able to tell the difference between “workflow exists”, “workflow ran”, and “workflow is trustworthy enough to declare Phase III complete”

## Acceptance Scenarios

Phase III should not be called complete until these scenarios pass:

1. Successful grounded council run
- run completes in fixed Gemini -> Claude -> Codex order
- all required artifacts exist
- `run.json` shows succeeded stages and terminal `completed`
- `convergence.json` shows a converged result with extracted unresolved risks
- trace writeback succeeds

2. Retry after transient stage failure
- one stage fails once, retries, then succeeds
- attempt history is preserved in `run.json`
- final run remains diagnosable and self-consistent

3. Timeout or stall on a stage
- a stage exceeds timeout
- process is terminated
- attempt status records timeout explicitly
- run ends as `failed` or `incomplete` according to defined rules
- artifacts still show enough evidence to explain the stall

4. Terminal failure after retries exhausted
- a stage repeatedly fails or returns empty output
- run stops cleanly
- downstream stages do not execute
- persisted state accurately reflects the failure

5. Grounded run with GitNexus targets
- a run invoked with `--targets` includes GitNexus impact context in prompts and artifacts
- recorded graph context is present in `run.json`
- the council output meaningfully references graph evidence rather than dropping it

6. Grounded run with curated memory continuity
- retrieved memory evidence is present in context artifacts
- final decision uses or at least reflects that memory grounding
- promoted durable records, if chosen, match canonical curated shapes

7. Non-converged but honest outcome
- final stage produces usable text but does not meet convergence contract
- run is not mislabeled as complete
- operator-visible output says convergence was not achieved

## Artifact Correctness Requirements

For a council run to count as valid:

- every stage must have a prompt artifact
- every attempt must have stdout/stderr artifact paths recorded
- every recorded artifact path must exist on disk
- `run.json` must be parseable and internally consistent
- `convergence.json` must be parseable and consistent with `run.json`
- trace records must reference the correct run id, route, stage statuses, and artifacts directory
- failed runs must preserve the same minimum diagnosis surface as successful runs, except for artifacts that logically cannot exist because execution never reached that point

## Liveness, Retry, and Stall Validation

Minimum validation still required:

- test successful single-attempt execution
- test transient failure followed by recovery
- test timeout/stall termination
- test empty-output failure
- test retries exhausted without convergence
- test that later stages do not run after unrecoverable earlier-stage failure
- test persisted state transitions across `pending -> running -> retrying/failed/succeeded`

Phase III is not complete if timeout/stall behavior remains only assumed instead of tested.

## Grounding Validation

### Curated Memory

Must prove:

- council context includes retrieved memory evidence from Layers’ memory path
- durable conclusions can be promoted into canonical curated records without ad hoc schema drift
- promoted records round-trip through existing curated retrieval behavior

### GitNexus Artifacts

Must prove:

- `--targets` produces structured GitNexus impact context in the run record
- prompts and final artifacts preserve that graph grounding
- council output can surface graph-backed implications without pretending to be broader repo orchestration

## Convergence Validation

Convergence should be considered valid only if:

- the final stage produces the required decision structure
- unresolved risks are captured explicitly
- next steps are actionable
- the output meets the declared convergence contract
- the resulting terminal state in `run.json` and `convergence.json` agrees

If the model returns text that is useful but not converged, that should count as recorded evidence, not successful closure.

## Operator Honesty And Status Reporting Requirements

To call Phase III complete honestly:

- `validate` must stay a readiness/configuration check, not a closure proof
- the council command output must distinguish:
  - run succeeded and converged
  - run executed but did not converge
  - run failed operationally
- status text must not hide retries, stalls, or degraded grounding
- documentation must state what is guaranteed by artifacts and what still depends on external model commands

The user/operator should never have to infer whether a run is trustworthy from raw JSON alone.

## Pass/Fail Criteria For Declaring Phase III Complete

### Pass

Phase III can be called complete only when all of the following are true:

- council runs have a defined and validated artifact contract
- convergence is recorded through a reliable, tested contract rather than a weak free-text heuristic alone
- timeout, retry, empty-output, and retry-exhaustion behaviors are covered by tests
- grounded runs with curated memory and GitNexus targets are validated end to end
- completed council outcomes can be explicitly promoted into canonical curated memory records
- operator-visible status reporting clearly distinguishes completion, incompletion, and failure
- `cargo test` and `cargo run -- validate` remain green after the above hardening

### Fail

Phase III is not complete if any of the following remain true:

- convergence can still be marked by formatting luck
- timeout/stall behavior is untested
- durable conclusions remain trapped in traces without canonical promotion
- grounded council behavior is claimed but not validated
- status/reporting can still overstate what the workflow proves

## Explicit Deferrals That Do Not Block Honest Completion

These may remain out of scope without lying about Phase III completion:

- first-party provider adapters for Gemini, Claude, or Codex
- background supervisors or resumable long-running orchestration
- additional workflow types such as handoff or postmortem commands
- generalized multi-agent abstractions
- automatic promotion of every council trace into curated memory
- broader product-management or task-tracking features

## Recommended Execution Order

1. artifact contract hardening
2. convergence contract hardening
3. liveness/stall/retry tests
4. curated-memory promotion path
5. grounding validation for curated memory and GitNexus
6. operator honesty/status cleanup
7. final pass/fail review against this checklist
