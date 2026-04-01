# Layers Post-Core Roadmap

Date: 2026-03-31
Status: saved strategic roadmap

## North Star

The north star for Layers is:

> A user gives a directive, walks away, and comes back to a completed result.

This requires more than memory, graph context, and councils. It requires trustworthy unattended execution.

## Current Core (Phases I-VI)

The current narrowed Layers core is focused on:
- councils
- Memoryport continuity
- GitNexus code understanding
- effectiveness measurement

These are necessary foundations, but not sufficient for unattended completion.

## What must come after the current core

To reach the north star, Layers needs a second-stage execution substrate that adds:
- task decomposition
- durable execution state
- truthful progress tracking
- automatic recovery
- verified completion

This should be built **after** the current core is genuinely complete, not mixed prematurely into the core phases.

---

## Post-Core Track: Directive -> Done

### Mission

Enable Layers to accept a high-level directive, compile it into executable phases, run them with durable state, recover from stalls/failures where possible, and stop only on verified completion or truthful escalation.

---

## Phase VII — Execution Ledger / Durable Task State

### Goal
Create a durable source of truth for active work so unattended execution has real state instead of chat-dependent intent.

### Capabilities
- structured execution records
- task/phase status model
- dependency tracking
- artifact expectations
- retry/failure counters
- timestamps and liveness metadata

### Suggested states
- queued
- running
- retrying
- blocked
- degraded
- awaiting_verification
- complete
- failed
- escalated

### Why this matters
This prevents the system from claiming progress based on intention instead of evidence.

### Definition of done
- active work can be represented without chat state
- each phase/task has explicit status + expected artifacts
- execution state survives process/session boundaries

---

## Phase VIII — Directive Compiler

### Goal
Turn a user directive into a tracked execution plan automatically.

### Capabilities
- parse directive into scoped task
- identify workflows/providers needed
- produce phased execution plan
- assign expected artifacts and success criteria
- identify dependencies and risk level

### Inputs
- user directive
- curated memory
- graph context
- repo state

### Outputs
- execution plan
- phase list
- artifact expectations
- completion criteria

### Why this matters
This is what removes the need for the user to manually step the system through phases.

### Definition of done
- a non-trivial directive can be converted into an explicit tracked plan
- the plan is durable and inspectable
- expected artifacts and success criteria are explicit

---

## Phase IX — Autonomous Progression + Recovery

### Goal
Let Layers advance work on its own and recover from common failure modes without human babysitting.

### Capabilities
- step runner / phase executor
- liveness checks
- artifact advancement checks
- retry policy
- degradation rules
- escalation policy

### Failure modes to handle
- provider transient failure
- dead/stalled process
- missing artifact
- empty artifact
- config mismatch
- repo unprepared

### Why this matters
This is the difference between an impressive tool and something a user can actually trust while away.

### Definition of done
- common transient failures are retried/recovered automatically
- stalls are detected reliably
- false “in progress” claims are eliminated
- escalation happens only when recovery policy is exhausted

---

## Phase X — Completion Gate / Verified Done

### Goal
Ensure Layers knows when work is truly done.

### Capabilities
- completion criteria evaluation
- artifact existence checks
- test/build/validate gate integration
- quality/consistency checks
- truthful result summary

### Completion should require evidence such as
- deliverables exist
- final artifact is coherent/non-empty
- validation/tests/build passed where required
- expected workflow artifacts were produced
- completion criteria satisfied

### Why this matters
Without this, unattended execution becomes sophisticated wandering.

### Definition of done
- Layers can distinguish complete vs partial vs failed vs escalated
- “done” is evidence-backed
- final result summaries reflect real state honestly

---

## Guiding Principles for Post-Core Work

1. **No fake autonomy theater**
   - Build trustworthy delegation, not hype-driven “autonomy.”

2. **State before magic**
   - Durable task state and evidence matter more than clever prompt loops.

3. **Evidence over intention**
   - No status claim without liveness/artifact proof.

4. **Recovery before escalation**
   - The system should handle common failures itself.

5. **Verified done or honest escalation**
   - Those are the only acceptable terminal states.

6. **Build on the narrowed core**
   - Councils, Memoryport, and GitNexus remain foundational; post-core work should compose them, not replace them.

---

## Anti-Goals

Do not let post-core work become:
- generalized agent platform theater
- broad plugin marketplace ambitions
- endless workflow-engine abstraction
- fake human replacement ideology
- a system that claims autonomy but still needs hidden babysitting

---

## Relationship to Triumvirate

This roadmap is for Layers as builder tooling/proving ground.
If successful, the resulting patterns may later be promoted into Triumvirate.
But Layers should treat post-core execution discipline as a way to accelerate and de-risk Triumvirate, not as a competing end-state architecture.

---

## Practical sequencing

1. Finish current core (Phases I-VI)
2. Add durable execution ledger (Phase VII)
3. Add directive compiler (Phase VIII)
4. Add autonomous progression + recovery (Phase IX)
5. Add verified completion gate (Phase X)

Only then can Layers claim to support the north star in a serious way.

---

## Summary

The current Layers core gives us:
- better councils
- better continuity
- better codebase understanding

The post-core roadmap adds what is still missing for the real north star:
- durable execution state
- automatic progression
- recovery
- verified completion

That is the path from:
- useful tool

to
- trustworthy unattended execution substrate.
