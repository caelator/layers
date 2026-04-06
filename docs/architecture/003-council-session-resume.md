# RFC: Session Resume / Checkpoint for Council Runs

**Status:** Draft
**Date:** 2026-04-06
**Source:** Inspired by Ralph's `--resume` and AoE's tmux session persistence

---

## Problem

If a `layers council` run dies mid-deliberation (SIGTERM, crash, network failure), all work is lost. The council starts from scratch on the next invocation. For long-running deliberations, this is expensive and frustrating.

---

## Design

### CouncilRunCheckpoint

A checkpoint is a serialized snapshot of a council run's state, written after each stage completes.

```rust
#[derive(Serialize, Deserialize)]
pub struct CouncilRunCheckpoint {
    pub run_id: String,
    pub task: String,
    pub created_at: String,
    pub last_modified: String,
    pub current_stage_index: usize,
    pub stages_completed: Vec<CompletedStage>,
    pub convergence_state: ConvergenceState,
    pub context_payload: Option<serde_json::Value>,
    pub schema_version: u32,
}

#[derive(Serialize, Deserialize)]
pub struct CompletedStage {
    pub stage_name: String,
    pub output: String,
    pub artifacts_dir: PathBuf,
    pub outcome: StageOutcome,
    pub duration_ms: u64,
}

#[derive(Serialize, Deserialize)]
pub enum ConvergenceState {
    Unknown,
    Inconclusive { rounds_without_progress: u32 },
    Converged { plan: String },
    Failed { reason: String },
}
```

### Checkpoint Storage

Checkpoints stored in the data layer (or filesystem if data layer not yet built):

```
~/.layers/council-runs/<run_id>/checkpoint.json
~/.layers/council-runs/<run_id>/stages/stage-0/output.md
~/.layers/council-runs/<run_id>/stages/stage-1/output.md
...
```

### Resume Logic

```rust
pub fn execute_council_run_resume(run_id: &str) -> Result<CouncilRunRecord> {
    let checkpoint = CouncilRunCheckpoint::load(run_id)?;
    
    // Verify the run is resumable (not already converged or failed)
    match &checkpoint.convergence_state {
        ConvergenceState::Converged { .. } => {
            return Err(anyhow::anyhow!("Run {} already converged", run_id));
        }
        ConvergenceState::Failed { .. } => {
            return Err(anyhow::anyhow!("Run {} failed, cannot resume", run_id));
        }
        _ => {}
    }
    
    // Resume from the next uncompleted stage
    let next_stage_index = checkpoint.current_stage_index;
    let mut ctx = CouncilContext::from_checkpoint(&checkpoint)?;
    
    for (i, stage) in stages.iter().enumerate().skip(next_stage_index) {
        let outcome = execute_stage(stage, &mut ctx)?;
        checkpoint.record_stage(i, &outcome);
        checkpoint.save()?;  // save after each stage
        
        if outcome.is_terminal() {
            break;
        }
    }
    
    checkpoint.finalize()
}
```

### CLI Interface

```bash
# List recent council runs (with status)
layers council --list

# Resume a specific run
layers council --resume <run_id>

# Resume the most recent incomplete run
layers council --resume-last

# Show what a run's current state is without resuming
layers council --status <run_id>
```

### Checkpoint Written After Each Stage

The key invariant: **after every stage completes, a checkpoint is written before the next stage begins**. This means at worst, a crash loses exactly one stage of work.

### Graceful Shutdown

On SIGTERM, the running stage should:
1. Complete its current work
2. Write the checkpoint
3. Exit cleanly

```rust
// In council execution:
let outcome = execute_stage(stage, &mut ctx)?;
// SIGTERM during stage = partial work lost, but last checkpoint is valid
checkpoint.record_stage(current_index, &outcome);
checkpoint.save()?;
```

---

## Checkpoint Compatibility

| Scenario | Behavior |
|---|---|
| Run crashes mid-stage | Checkpoint reflects last completed stage; resume replays from there |
| Run times out | Checkpoint updated with timeout reason; resume detects and refuses |
| Schema version mismatch on resume | Checkpoint carries schema_version; fail fast with migration hint |
| Artifacts dir deleted on resume | Fail with "run {} is corrupted: missing artifacts" |

---

## Interaction with Circuit Breaker

If the circuit breaker trips during a resumed run, the checkpoint records the trip reason. On resume, the circuit breaker state is NOT restored — each run starts with a fresh breaker. This is intentional: we want a fresh evaluation, not an already-tripped breaker blocking a new attempt.

---

## Out of Scope

- Checkpoint compression (stages outputs can be large; defer to later)
- Checkpoint GC / retention policy (define later; default: keep all until explicitly pruned)
- Multi-runfork (resuming into a branch, exploring alternatives) — future work
