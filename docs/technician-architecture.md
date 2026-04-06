# Layers Technician — Architectural Design

> A resident self-healing subsystem inside Layers that ensures every plugin integration stays wired, healthy, and correct at all times.

**Status:** Design document  
**Author:** Architectural council (Caelator subagent)  
**Repo:** `~/Documents/GitHub/layers`  
**Based on:** `src/plugins/`, `src/council/`, `src/plugins/telemetry/`, `src/plugins/rlef/`, `src/cmd/monitor.rs`, `autonomous-monitor` SKILL.md  
**Note:** Council/circuit-breaker fix (commit `44f2adb`) is on the current branch and is incorporated as the baseline.

---

## 1. Problem Statement

Layers integrates three external systems — **MemoryPort (uc)**, **GitNexus**, and the **Council pipeline** — and owns two internal plugins (**rlef**, **telemetry**). These integrations are invoked by `handle_query` and `handle_council_run` in long CLI invocations, but:

- No component continuously verifies that integrations remain operational **between** CLI calls.
- JSONL artifact files accumulate over time and can grow corrupt (partial lines, inconsistent schema).
- The Council circuit breaker (commit `44f2adb`) correctly stops infinite loops but does not **heal** the underlying cause or **escalate** when retries are exhausted.
- UC/GitNexus can become unavailable silently between runs, causing degraded routing without any operator awareness.
- There is no concept of "plugin contract" — plugins like `rlef` and `telemetry` are statically compiled but have no health gate.

**The technician** closes this gap. It is not a separate service; it is a resident subsystem that runs inside Layers, continuously monitoring the wiring between Layers and every plugin/integration, self-healing what it can, and escalating what it cannot.

---

## 2. Responsibilities and Non-Responsibilities

### ✅ Technician IS Responsible For

- **Plugin registration verification** — confirming that `rlef` and `telemetry` plugin structs are instantiated and callable.
- **Integration liveness checks** — verifying `uc` binary + config exists before query; verifying `gitnexus` binary + index exists before council.
- **Artifact integrity** — detecting corrupt JSONL lines in `council-runs/`, `council-traces.jsonl`, and `layers-audit.jsonl`.
- **Council circuit-breaker state monitoring** — tracking `CouncilRunRecord.status == "failed"` runs and distinguishing transient vs. persistent failure.
- **Repair of fixable faults** — JSONL line truncation, config stub-creation, re-triggering of failed stages.
- **Escalation signaling** — writing structured escalation records when faults exceed repair budgets.
- **Health snapshot emission** — writing a technician health report alongside the telemetry plugin's `IntegrationHealthReport`.

### ❌ Technician Is NOT Responsible For

- Fixing Rust code bugs in Layers core (delegate to a fix subagent, not self-heal).
- Building or testing (delegate to `layers monitor` / OpenClaw cron).
- Git synchronization (handled by `layers monitor`).
- Changing plugin algorithmic behavior (e.g., RLEF charge parameters — those are contractual).
- Human escalation delivery (writes the record; OpenClaw cron handles notification).

---

## 3. Architecture

```
src/technician/
├── mod.rs              — public API, run_technician_cycle()
├── detection/
│   ├── mod.rs          — signal detectors
│   ├── plugin.rs       — plugin registration & contract checks
│   ├── uc.rs           — UC binary/config availability
│   ├── gitnexus.rs     — GitNexus binary/index availability
│   ├── council.rs      — council run record & circuit-breaker checks
│   └── artifacts.rs    — JSONL integrity checks
├── repair/
│   ├── mod.rs          — repair orchestration, dry-run gate, budget
│   ├── jsonl.rs        — corrupt-line rollback
│   ├── config.rs       — UC config stub creation
│   ├── council.rs      — re-trigger failed council stages
│   └── reset.rs        — circuit-breaker state reset
├── escalation/
│   ├── mod.rs          — escalation policy engine
│   └── records.rs      — EscalationRecord persistence
└── data/
    ├── mod.rs          — TechnicianState, RepairHistory, HealthSnapshot
    ├── state.rs        — persistence to ~/.layers/technician-state.json
    └── history.rs      — repair log to ~/.layers/technician-repairs.jsonl
```

**Relationship to existing modules:**
- `src/plugins/telemetry/` continues to own `RoutingDecision` event emission. The technician writes a parallel `TechnicianHealth` event to `~/.layers/technician-health.jsonl`.
- `src/cmd/monitor.rs` continues to own git/build/test monitoring. The technician is orthogonal — it focuses on plugin/integration wiring, not repo health.
- `src/council/` is read by the technician (via persisted `CouncilRunRecord` JSON files). The technician never imports `execute_council_run` — it triggers repairs but does not replace the council runner.

---

## 4. Detection Model

The technician watches six signal families, each with a detector function that returns `Vec<Diagnosis>`:

### Signal 1 — Plugin Registration
Check: `rlef::RlefRouterPlugin::new().select(["a","b"])` and `TelemetryPlugin::new(&tempdir).record_routing_decision(...)` can be called without panic.

**Diagnosis types:**
- `PluginPanic` — plugin constructor or method panics (RLEF floor mismatch, etc.)
- `PluginNotCompiling` — caught at compile time (static module structure)

### Signal 2 — UC Availability
Check: `uc::is_available()` returns `true`; `uc retrieve` spawns and exits 0 within 500ms.

**Diagnosis types:**
- `UcBinaryMissing` — `which("uc")` false
- `UcConfigMissing` — `uc_config_path()` does not exist
- `UcTimeout` — spawn succeeds but no exit within 500ms
- `UcNonZeroExit` — binary exits non-zero on `retrieve` call

### Signal 3 — GitNexus Availability
Check: `gitnexus` binary on PATH; `gitnexus index --status` succeeds or `git rev-parse --git-dir` on indexed repos succeeds.

**Diagnosis types:**
- `GitNexusBinaryMissing`
- `GitNexusIndexStale` — index hasn't been updated in >48h (compare file mtime)
- `GitNexusIndexMissing` — repo not in gitnexus's tracked list

### Signal 4 — Council Artifact Integrity
Check: iterate all `council-runs/<run-id>/` directories; parse `run.json`; check all referenced `stdout_path`/`stderr_path` files exist; validate JSONL lines in `council-traces.jsonl`.

**Diagnosis types:**
- `CouncilRunArtifactsMissing` — referenced file does not exist
- `CouncilRunJsonCorrupt` — `run.json` fails to parse
- `CouncilTracesJsonlCorrupt` — JSONL contains partial or malformed lines
- `CouncilAuditJsonlCorrupt`

### Signal 5 — Circuit Breaker Exhaustion
Check: read all `CouncilRunRecord` with `status == "failed"` from the last 7 days; count `circuit_breaker.tripped()` occurrences per unique task pattern.

**Diagnosis types:**
- `CircuitBreakerTripped` — threshold exceeded, run terminated
- `StageRetriesExhausted` — `status_reason == "retries_exhausted"`
- `StageTimedOut` — `status_reason == "stage_timed_out"`
- `ConvergenceNotReached` — `status == "incomplete"` after all stages run

### Signal 6 — Telemetry Plugin Health
Check: read `telemetry/events.jsonl`; compute aggregate stats (event count, error rate, average latency); compare to thresholds.

**Diagnosis types:**
- `TelemetryFileMissing` — events file does not exist (telemetry not initialized)
- `TelemetryErrorRateHigh` — error rate > 20% in last 100 events
- `TelemetryLatencySpike` — average latency > 2× rolling 7-day baseline

---

## 5. Repair Model

Each diagnosis maps to zero or more repair actions. Repairs have a **dry-run mode** (default ON for Phase 1) and a **repair budget** (max N repairs per cycle per diagnosis type).

### Repair Action Taxonomy

| Diagnosis | Repair | Autonomous? | Dry-Run? |
|---|---|---|---|
| `CouncilTracesJsonlCorrupt` | Truncate file at last valid JSON line | ✅ Yes | ✅ Yes |
| `CouncilAuditJsonlCorrupt` | Truncate at last valid JSON line | ✅ Yes | ✅ Yes |
| `CouncilRunArtifactsMissing` | Log warning, no repair | ✅ Yes | ✅ Yes |
| `UcConfigMissing` | Stub `~/.memoryport/uc.toml` with empty `[uc]` section | ✅ Yes | ✅ Yes |
| `UcTimeout` | Increment failure counter; log | ✅ Yes | ✅ Yes |
| `UcNonZeroExit` | Increment failure counter | ✅ Yes | ✅ Yes |
| `CircuitBreakerTripped` | Reset circuit breaker for that task pattern | ✅ Yes | ✅ Yes |
| `StageRetriesExhausted` | Re-trigger stage via `execute_council_run` with `--retry-limit` bumped | ⚠️ Escalate first | ✅ Yes |
| `TelemetryErrorRateHigh` | Flag for human review | ❌ No | N/A |
| `GitNexusIndexStale` | Run `layers refresh` subprocess | ⚠️ Escalate | ✅ Yes |
| `PluginPanic` | Flag for human review | ❌ No | N/A |

### Repair Budget Rules
- **JSONL truncate**: max 3 lines removed per cycle (prevents runaway truncation)
- **UC config stub**: max 1 stub per cycle
- **Circuit breaker reset**: max 2 per task-pattern per hour
- **Council re-trigger**: 0 autonomous (always escalate)

### Rollback Safety
All file mutations are performed on a temp copy first; the copy is validated (parse JSON lines) before being renamed over the original (atomic swap via `rename(2)` on POSIX).

---

## 6. Runtime Model

### Option A — Cron-triggered one-shot (recommended for MVP)

The technician runs as `cargo run --quiet -- technician run` invoked by an OpenClaw cron job every **5 minutes** (same interval as the autonomous-monitor). Each invocation runs one full detection cycle, applies repairs within budget, writes state, and exits. No daemon process.

**Advantages:** Matches the existing `layers monitor run` pattern; no lock management complexity; survives machine restarts; integrates with the existing OpenClaw cron scheduler.

```bash
openclaw cron add \
  --name layers-technician \
  --every-ms 300000 \
  --sessionTarget isolated \
  --delivery none \
  -- cargo run --quiet -- technician run
```

**Lock file:** `~/.layers/.technician.lock` (same pattern as `~/.layers/.monitor.lock`).

### Option B — In-process background thread

A `LazyLock<Arc<Mutex<TechnicianEngine>>>` inside the CLI process that spawns a background thread on first `handle_query` call. The thread runs a loop with `sleep(300)` between cycles.

**Advantages:** Reacts within a single long CLI session.

**Disadvantages:** CLI process must stay alive; lifecycle management is complex; does not survive brief CLI invocations (`layers query ...`).

### Decision: Option A (Cron-triggered one-shot)

The technician's mission is **between-run integrity**, not intra-run reactivity. A 5-minute cron cycle is sufficient because:
- JSONL corruption accumulates over time, not milliseconds.
- UC/GitNexus outages persist for minutes at a time.
- Council runs take minutes to complete anyway.

For intra-run protection, Layers uses the existing circuit breaker (`src/council/circuit_breaker.rs`) and telemetry plugin — the technician augments them, it does not replace them.

---

## 7. Safety and Escalation Rules

### Escalation Triggers (always write `TechnicianEscalationRecord`)
1. Same diagnosis type occurs **3 or more times** in rolling 24-hour window → escalate.
2. Any `StageRetriesExhausted` or `StageTimedOut` diagnosis → escalate immediately.
3. `TelemetryErrorRateHigh` persists **2 consecutive cycles** → escalate.
4. Any repair **fails** (validation of repaired artifact fails) → escalate immediately.
5. `GitNexusIndexStale` + `layers refresh` fails → escalate.

### Escalation Record Schema
```json
{
  "ts": "2026-04-06T09:17:00Z",
  "cycle_id": "tech-20260406t0917",
  "diagnosis": "StageRetriesExhausted",
  "context": {
    "run_id": "council-...-some-task",
    "stage": "codex",
    "status_reason": "retries_exhausted",
    "artifacts_dir": "/Users/bri/.memoryport/council-runs/council-...-some-task"
  },
  "repair_attempted": true,
  "repair_outcome": "escalate_only",
  "escalation_reason": "human_required: council stage exhausted all retries"
}
```

### Safety Invariants (never violated)
- Technician never deletes a `council-runs/<run-id>/` directory.
- Technician never modifies `run.json` or `convergence.json` directly (only truncates `council-traces.jsonl` and `layers-audit.jsonl` at valid boundaries).
- Technician never runs `cargo build` or `cargo test` (that is `layers monitor`'s job).
- Technician never sends external notifications directly — it writes records that OpenClaw cron/agent consumes.

---

## 8. Data Model and Artifacts

### File Layout (`~/.layers/`)
```
.layers/
  .technician.lock              — flock lock file (same pattern as .monitor.lock)
  technician-state.json         — current TechnicianState (last cycle ts, health scores)
  technician-repairs.jsonl      — append-only repair action log
  technician-escalations.jsonl  — append-only escalation record log
  .technician-health.jsonl      — periodic TechnicianHealth snapshots (parallel to telemetry/)
```

### TechnicianState Schema
```json
{
  "last_cycle_ts": "2026-04-06T09:17:00Z",
  "cycle_id": "tech-20260406t0917",
  "uc_available": true,
  "gitnexus_available": true,
  "telemetry_event_count": 142,
  "telemetry_error_rate": 0.03,
  "council_runs_total": 8,
  "council_runs_failed_7d": 1,
  "pending_escalations": 0,
  "diagnoses_this_cycle": ["UcTimeout", "CircuitBreakerTripped"],
  "repairs_this_cycle": 1,
  "repair_budget_remaining": {"jsonl_truncate": 2, "uc_stub": 1}
}
```

### Health Snapshot (TechnicianHealth)
Parallel to `RoutingDecisionEvent`, emitted every cycle:
```json
{
  "schema_version": "1.0",
  "ts": "2026-04-06T09:17:00Z",
  "cycle_id": "tech-20260406t0917",
  "uc_ok": true,
  "gitnexus_ok": true,
  "telemetry_ok": true,
  "council_healthy_runs_7d": 7,
  "council_failed_runs_7d": 1,
  "active_escalations": 0,
  "diagnoses": ["UcTimeout"],
  "repairs_applied": ["jsonl_truncate:1"]
}
```

---

## 9. Concrete Implementation Phases

### Phase 0 — Scaffold (1 day)
- [ ] Create `src/technician/mod.rs` with public `run_technician_cycle() -> Result<CycleReport>` function.
- [ ] Create `src/technician/detection/mod.rs` with `Diagnosis`, `DiagnosisType`, `Severity` enums.
- [ ] Create `src/technician/data/mod.rs` with `TechnicianState`, `RepairRecord`, `EscalationRecord` structs.
- [ ] Add `Technician` CLI command to `src/main.rs` with `Run` and `Status` subcommands (mirrors `Monitor`).
- [ ] Implement `~/.layers/.technician.lock` flock acquisition — copy pattern from `src/cmd/monitor.rs`.
- [ ] Wire `TechnicianState::load()` and `TechnicianState::persist()`.

### Phase 1 — Detection Only, No Repair (2 days)
- [ ] Implement `detect_uc_availability()` — calls `uc::is_available()`, spawns test `uc retrieve`.
- [ ] Implement `detect_gitnexus_availability()` — `which("gitnexus")`, mtime check on index.
- [ ] Implement `detect_council_artifacts()` — iterates `council-runs/` directories, validates `run.json`, checks file references.
- [ ] Implement `detect_circuit_breaker_exhaustion()` — reads recent `CouncilRunRecord` files with `status == "failed"`.
- [ ] Implement `detect_telemetry_health()` — reads `telemetry/events.jsonl`, computes error rate + latency stats.
- [ ] Implement `run_technician_cycle()` — orchestrates all detectors, builds `CycleReport`, persists state.
- [ ] Add `layers technician run` → dry-run only, writes report to stdout.

### Phase 2 — Repair Actions (2 days)
- [ ] Implement `RepairEngine::can_repair(d: &Diagnosis) -> bool` and `repair_budget()`.
- [ ] Implement `repair_jsonl_truncate(path: &Path) -> usize` — finds last valid JSON line, atomically replaces.
- [ ] Implement `repair_uc_config_stub()` — creates `uc.toml` with `[uc]` section if missing.
- [ ] Implement `repair_circuit_breaker_reset(run_id: &str)` — clears no-progress counter for task pattern.
- [ ] Wire repair engine into `run_technician_cycle()` behind `DRY_RUN` env flag (default: true).
- [ ] Add `layers technician run --apply` flag to enable actual repairs.

### Phase 3 — Escalation Engine (1 day)
- [ ] Implement `EscalationPolicy::evaluate(cycle: &CycleReport, history: &RepairHistory) -> Vec<EscalationRecord>`.
- [ ] Implement rolling 24-hour window counter per diagnosis type (using timestamps in `technician-repairs.jsonl`).
- [ ] Write `technician-escalations.jsonl` on escalation trigger.
- [ ] Implement `layers technician status` → prints current state and recent escalations.
- [ ] Wire escalation into OpenClaw notification: spawn a fix subagent when escalation count > 0 (same pattern as `spawn_fix_subagent` in `cmd/monitor.rs`).

### Phase 4 — Telemetry Integration and Hardening (1 day)
- [ ] Emit `TechnicianHealth` snapshots to `~/.layers/.technician-health.jsonl`.
- [ ] Add technician-specific entries to `layers telemetry report` output.
- [ ] Register `layers technician run` as OpenClaw cron job (every 5 minutes).
- [ ] Write integration tests: simulate corrupt JSONL, verify truncate repair; simulate UC missing, verify stub repair.
- [ ] Write repair rollback test: corrupt after repair, verify cycle detects it again.

---

## 10. MVP Recommendation

**MVP = Phase 0 + Phase 1 + minimal Phase 2 (JSONL truncate only)**

Deliver the minimum viable technician in **3 days of work**:

```
layers technician run --dry-run
```

This runs all five detection suites, prints a human-readable `CycleReport`, persists `TechnicianState`, and writes `technician-repairs.jsonl` (empty in dry-run). No file mutations occur.

**What this achieves:**
- Full visibility into the integration health state on every cron tick.
- No risk of data loss from premature repairs.
- Foundation for Phase 2 repair layer to build on safely.
- The `CycleReport` output can be consumed by OpenClaw agent/subscribers without any new notification infrastructure.

**Enable repairs incrementally:** Pass `--apply` only after Phase 2 JSONL truncate is tested in dry-run for 3+ cron cycles without false positives.

---

## 11. Open Questions and Risks

### Open Questions

1. **How should the technician handle council runs that are mid-execution?**
   If a `CouncilRunRecord` has `status == "running"` and its directory exists, the technician should **skip** that run-id entirely (do not repair mid-flight runs). Confirm: should `status == "running"` with a stale `updated_at` (>30 min) be treated as a crash? Recommendation: yes, treat as `StageTimedOut` and escalate.

2. **Should the technician track and repair `route-corrections.jsonl`?**
   The router's `correction_cache` loads from `route-corrections.jsonl`. If that file is corrupt, routing quality degrades silently. Recommend: yes, add a 7th detection signal for `route-corrections.jsonl` integrity in Phase 3.

3. **Should the technician auto-expire old council runs instead of archiving?**
   Currently `cmd/monitor.rs` archives runs >7 days old. The technician could additionally **prune** failed runs older than 30 days. This is safe if the `convergence.json` summary has been promoted to curated memory first. Recommend: add as Phase 3 option.

4. **What is the schema version strategy for technician artifacts?**
   Parallel to `CONTEXT_PAYLOAD_SCHEMA_VERSION` in `config.rs`, introduce `TECHNICIAN_SCHEMA_VERSION` in `technician/data/mod.rs`. All JSON artifacts include `schema_version` field.

5. **Can the technician and `layers monitor` run in the same cron slot?**
   Yes — they target different concerns and have separate lock files. No conflict.

### Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| JSONL truncate removes valid data | Low | Medium | Max 3 lines/cycle; validate parsed lines before atomic swap |
| Technician lock not released on crash | Low | Medium | Lock is `flock(2)` — kernel releases on any process exit |
| Repair of in-flight council run corrupts artifacts | Low | High | Skip runs with `status == "running"`; check `updated_at` staleness |
| Escalation loop (repair fails → escalate → repair again) | Medium | Low | Escalation budget: max 1 per diagnosis per 6 hours |
| `gitnexus index` mtime check is unreliable on NFS/VirtualBox | Low | Medium | Fall back to checking `.gitnexus/meta.json` mtime instead of repo `.git` mtime |
| UC stub config masks real UC failure | Low | Low | Stub is `[uc]` with no fields; `uc::is_available()` will still return false if binary missing |

---

## Appendix: Council Circuit-Breaker Fix (44f2adb) — Relationship to Technician

Commit `44f2adb` fixed the council circuit breaker to **record both successful and failed stages** toward the no-progress counter. This is the correct behavior — a stage that exits non-zero still consumes a round and should count.

The technician's relationship to this fix:
- The technician **observes** the circuit breaker's effect: it reads `CouncilRunRecord` files where `status_reason` contains `"circuit breaker tripped after N rounds"`.
- The technician does **not** re-implement the circuit breaker — that logic lives in `src/council/circuit_breaker.rs`.
- The technician can **reset** the circuit breaker state for a given task pattern (by re-triggering the run, which creates a fresh `CircuitBreaker` instance with counters at zero).

This separation is intentional: the circuit breaker is a **per-run** guard; the technician is a **cross-run** monitor and healer.
