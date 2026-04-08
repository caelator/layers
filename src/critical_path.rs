//! Critical-path routing V1.
//!
//! Implements weighted fair queueing with a reserved worker slot for
//! critical-path tasks.  A task is `critical_path=true` iff it sits on
//! the synchronous return path of a user-initiated prompt and the caller
//! awaits the result before replying.
//!
//! Council decisions baked in:
//! - 8:1 weighted dequeue ratio (critical : standard).
//! - 1 reserved worker slot for the critical queue.
//! - Bounded critical-queue depth with back-pressure.
//! - Synchronous sub-tasks inherit `critical_path=true`; async/background
//!   side-effects default to `false`.
//! - Missing flags default to `false` for backward compatibility.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Critical queue dequeues per scheduling round.
const CRITICAL_WEIGHT: usize = 8;
/// Standard queue dequeues per scheduling round.
const STANDARD_WEIGHT: usize = 1;
/// Default maximum depth for the critical queue before back-pressure kicks in.
const DEFAULT_CRITICAL_QUEUE_CAPACITY: usize = 64;
/// Default total worker count (including the reserved critical slot).
const DEFAULT_TOTAL_WORKERS: usize = 4;
/// The number of worker slots permanently reserved for critical work.
const RESERVED_CRITICAL_SLOTS: usize = 1;

// ---------------------------------------------------------------------------
// Classification helper
// ---------------------------------------------------------------------------

/// Returns `true` when the task sits on the synchronous return path of a
/// user-initiated prompt and the caller awaits the result before replying.
///
/// The function inspects the envelope flag first (explicit caller intent),
/// then falls back to heuristic detection based on the `route` string and
/// whether there is a parent task flagged as critical.
pub fn is_critical_path(
    explicit_flag: Option<bool>,
    route: &str,
    parent_critical: bool,
) -> bool {
    // 1. Explicit flag takes precedence (caller opted in/out).
    if let Some(flag) = explicit_flag {
        return flag;
    }

    // 2. Synchronous sub-tasks inherit from parent.
    if parent_critical {
        return true;
    }

    // 3. Routes that sit on the synchronous user-reply path.
    matches!(route, "direct" | "council_only" | "both" | "memory_only" | "graph_only")
}

/// Determine whether a sub-task should inherit the critical-path flag.
///
/// - Synchronous children: inherit `true` from a critical parent.
/// - Async / background side-effects: always `false`.
pub fn inherit_critical_path(parent_critical: bool, is_sync: bool) -> bool {
    if is_sync {
        parent_critical
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Queue item
// ---------------------------------------------------------------------------

/// Priority tier for queue classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    /// On the synchronous return path — latency-sensitive.
    Critical,
    /// Background / side-effect — best-effort.
    Standard,
}

/// A unit of work submitted to the dispatcher.
#[derive(Debug, Clone)]
pub struct TaskItem {
    /// Unique task identifier (typically `run_id`).
    pub id: String,
    /// Whether this task is on the critical path.
    pub critical_path: bool,
    /// Wall-clock instant when the item was enqueued.
    pub enqueued_at: Instant,
}

impl TaskItem {
    /// Create a new task item stamped with the current time.
    pub fn new(id: impl Into<String>, critical_path: bool) -> Self {
        Self {
            id: id.into(),
            critical_path,
            enqueued_at: Instant::now(),
        }
    }

    /// Convenience: classify into a [`Priority`].
    pub fn priority(&self) -> Priority {
        if self.critical_path {
            Priority::Critical
        } else {
            Priority::Standard
        }
    }
}

// ---------------------------------------------------------------------------
// Observability
// ---------------------------------------------------------------------------

/// Snapshot of queue metrics for structured logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMetrics {
    pub critical_depth: usize,
    pub standard_depth: usize,
    pub critical_capacity: usize,
    pub total_critical_enqueued: u64,
    pub total_standard_enqueued: u64,
    pub total_critical_dequeued: u64,
    pub total_standard_dequeued: u64,
    pub total_critical_rejected: u64,
    pub critical_wait_ms_p50: u64,
    pub critical_wait_ms_p99: u64,
    pub standard_wait_ms_p50: u64,
    pub standard_wait_ms_p99: u64,
}

/// Record emitted each time a task is dequeued, suitable for JSONL logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DequeueEvent {
    pub task_id: String,
    pub priority: String,
    pub wait_ms: u64,
    pub critical_depth_after: usize,
    pub standard_depth_after: usize,
}

// ---------------------------------------------------------------------------
// Back-pressure
// ---------------------------------------------------------------------------

/// Result of attempting to enqueue a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueResult {
    /// Task was accepted into the queue.
    Accepted,
    /// Critical queue is at capacity — caller should back off.
    BackpressureCritical,
}

// ---------------------------------------------------------------------------
// Weighted Fair Queue
// ---------------------------------------------------------------------------

/// Internal mutable state behind the mutex.
struct QueueState {
    critical: VecDeque<TaskItem>,
    standard: VecDeque<TaskItem>,
    /// Position within the current scheduling round.
    round_position: usize,
    /// Counters for observability.
    total_critical_enqueued: u64,
    total_standard_enqueued: u64,
    total_critical_dequeued: u64,
    total_standard_dequeued: u64,
    total_critical_rejected: u64,
    /// Recent wait times for percentile calculation (ring buffer).
    critical_waits: VecDeque<u64>,
    standard_waits: VecDeque<u64>,
    /// Whether the queue has been shut down.
    shutdown: bool,
}

/// Thread-safe weighted fair queue with bounded critical-queue depth.
///
/// Dequeue order follows an 8:1 weighted round-robin: for every scheduling
/// round the dispatcher pulls up to 8 critical items, then 1 standard item.
/// When either lane is empty the other lane's items are served immediately
/// (no starvation).
#[derive(Clone)]
pub struct WeightedFairQueue {
    state: Arc<Mutex<QueueState>>,
    not_empty: Arc<Condvar>,
    critical_capacity: usize,
}

impl WeightedFairQueue {
    /// Create a new queue with the given critical-queue capacity.
    pub fn new(critical_capacity: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(QueueState {
                critical: VecDeque::new(),
                standard: VecDeque::new(),
                round_position: 0,
                total_critical_enqueued: 0,
                total_standard_enqueued: 0,
                total_critical_dequeued: 0,
                total_standard_dequeued: 0,
                total_critical_rejected: 0,
                critical_waits: VecDeque::with_capacity(256),
                standard_waits: VecDeque::with_capacity(256),
                shutdown: false,
            })),
            not_empty: Arc::new(Condvar::new()),
            critical_capacity,
        }
    }

    /// Create a queue with default capacity.
    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_CRITICAL_QUEUE_CAPACITY)
    }

    /// Attempt to enqueue a task.  Returns [`EnqueueResult::BackpressureCritical`]
    /// when the critical queue has reached capacity.
    pub fn enqueue(&self, item: TaskItem) -> EnqueueResult {
        let mut state = self.state.lock().expect("queue lock poisoned");
        if item.critical_path {
            if state.critical.len() >= self.critical_capacity {
                state.total_critical_rejected += 1;
                return EnqueueResult::BackpressureCritical;
            }
            state.total_critical_enqueued += 1;
            state.critical.push_back(item);
        } else {
            state.total_standard_enqueued += 1;
            state.standard.push_back(item);
        }
        self.not_empty.notify_one();
        EnqueueResult::Accepted
    }

    /// Block until a task is available, then return it following the weighted
    /// fair schedule. Returns `None` after [`shutdown`] is called and both
    /// queues are drained.
    pub fn dequeue(&self) -> Option<(TaskItem, DequeueEvent)> {
        let mut state = self.state.lock().expect("queue lock poisoned");
        loop {
            if let Some(result) = Self::try_dequeue_inner(&mut state) {
                return Some(result);
            }
            if state.shutdown && state.critical.is_empty() && state.standard.is_empty() {
                return None;
            }
            state = self.not_empty.wait(state).expect("queue lock poisoned");
        }
    }

    /// Non-blocking dequeue — returns `None` immediately if nothing is ready.
    pub fn try_dequeue(&self) -> Option<(TaskItem, DequeueEvent)> {
        let mut state = self.state.lock().expect("queue lock poisoned");
        Self::try_dequeue_inner(&mut state)
    }

    /// Signal that no more items will be enqueued.  Workers will drain
    /// remaining items and then receive `None` from `dequeue()`.
    pub fn shutdown(&self) {
        let mut state = self.state.lock().expect("queue lock poisoned");
        state.shutdown = true;
        self.not_empty.notify_all();
    }

    /// Snapshot current metrics.
    pub fn metrics(&self) -> QueueMetrics {
        let state = self.state.lock().expect("queue lock poisoned");
        QueueMetrics {
            critical_depth: state.critical.len(),
            standard_depth: state.standard.len(),
            critical_capacity: self.critical_capacity,
            total_critical_enqueued: state.total_critical_enqueued,
            total_standard_enqueued: state.total_standard_enqueued,
            total_critical_dequeued: state.total_critical_dequeued,
            total_standard_dequeued: state.total_standard_dequeued,
            total_critical_rejected: state.total_critical_rejected,
            critical_wait_ms_p50: percentile(&state.critical_waits, 50),
            critical_wait_ms_p99: percentile(&state.critical_waits, 99),
            standard_wait_ms_p50: percentile(&state.standard_waits, 50),
            standard_wait_ms_p99: percentile(&state.standard_waits, 99),
        }
    }

    // -- internal ------------------------------------------------------------

    fn try_dequeue_inner(state: &mut QueueState) -> Option<(TaskItem, DequeueEvent)> {
        // Determine which lane to pull from based on round position.
        //
        // Round layout (total = CRITICAL_WEIGHT + STANDARD_WEIGHT = 9):
        //   positions 0..7 → critical
        //   position  8    → standard
        //
        // When the selected lane is empty, fall through to the other.
        let round_total = CRITICAL_WEIGHT + STANDARD_WEIGHT;
        let prefer_critical = state.round_position < CRITICAL_WEIGHT;

        let item = if prefer_critical {
            state
                .critical
                .pop_front()
                .or_else(|| state.standard.pop_front())
        } else {
            state
                .standard
                .pop_front()
                .or_else(|| state.critical.pop_front())
        };

        let item = item?;

        // Advance round position.
        state.round_position = (state.round_position + 1) % round_total;

        // Record wait time.
        let wait_ms = u64::try_from(item.enqueued_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        if item.critical_path {
            state.total_critical_dequeued += 1;
            push_ring(&mut state.critical_waits, wait_ms, 256);
        } else {
            state.total_standard_dequeued += 1;
            push_ring(&mut state.standard_waits, wait_ms, 256);
        }

        let event = DequeueEvent {
            task_id: item.id.clone(),
            priority: if item.critical_path {
                "critical".to_string()
            } else {
                "standard".to_string()
            },
            wait_ms,
            critical_depth_after: state.critical.len(),
            standard_depth_after: state.standard.len(),
        };

        Some((item, event))
    }
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// Configuration for the [`Dispatcher`].
#[derive(Debug, Clone)]
pub struct DispatcherConfig {
    /// Total number of worker slots (including reserved).
    pub total_workers: usize,
    /// Number of slots reserved exclusively for critical work.
    pub reserved_critical_slots: usize,
    /// Maximum depth of the critical queue before back-pressure.
    pub critical_queue_capacity: usize,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            total_workers: DEFAULT_TOTAL_WORKERS,
            reserved_critical_slots: RESERVED_CRITICAL_SLOTS,
            critical_queue_capacity: DEFAULT_CRITICAL_QUEUE_CAPACITY,
        }
    }
}

/// Tracks how many workers are currently active and on which lane.
struct WorkerAccounting {
    active_critical: usize,
    active_standard: usize,
}

/// Boundary routing dispatcher with reserved critical-path worker slot.
///
/// The dispatcher wraps a [`WeightedFairQueue`] and adds worker-slot
/// accounting: one slot is reserved so that critical work always has at
/// least one worker available, even when the pool is saturated with
/// standard tasks.
pub struct Dispatcher {
    queue: WeightedFairQueue,
    config: DispatcherConfig,
    accounting: Mutex<WorkerAccounting>,
}

impl Dispatcher {
    /// Create a dispatcher with the given configuration.
    pub fn new(config: DispatcherConfig) -> Self {
        let queue = WeightedFairQueue::new(config.critical_queue_capacity);
        Self {
            queue,
            config,
            accounting: Mutex::new(WorkerAccounting {
                active_critical: 0,
                active_standard: 0,
            }),
        }
    }

    /// Create a dispatcher with default settings.
    pub fn with_defaults() -> Self {
        Self::new(DispatcherConfig::default())
    }

    /// Submit a task.  Respects critical-queue back-pressure.
    pub fn submit(&self, item: TaskItem) -> EnqueueResult {
        self.queue.enqueue(item)
    }

    /// Attempt to claim a worker slot for the next available task.
    ///
    /// Returns `None` when:
    /// - The queue is empty (non-blocking variant), or
    /// - The worker-slot reservation policy would be violated (a standard
    ///   task was dequeued but all unreserved slots are busy).
    ///
    /// On success, the caller **must** call [`release`] when done.
    pub fn acquire(&self) -> Option<(TaskItem, DequeueEvent)> {
        let (item, event) = self.queue.try_dequeue()?;
        let mut acct = self.accounting.lock().expect("accounting lock poisoned");
        let total_active = acct.active_critical + acct.active_standard;

        if item.critical_path {
            // Critical work can always claim a slot (up to total_workers).
            if total_active >= self.config.total_workers {
                // All slots full — re-enqueue.
                let _ = self.queue.enqueue(item);
                return None;
            }
            acct.active_critical += 1;
        } else {
            // Standard work cannot use the reserved critical slot(s).
            let available_for_standard =
                self.config.total_workers.saturating_sub(self.config.reserved_critical_slots);
            if acct.active_standard >= available_for_standard {
                // Re-enqueue: the only free slots are reserved.
                let _ = self.queue.enqueue(item);
                return None;
            }
            if total_active >= self.config.total_workers {
                let _ = self.queue.enqueue(item);
                return None;
            }
            acct.active_standard += 1;
        }

        Some((item, event))
    }

    /// Release a worker slot after task completion.
    pub fn release(&self, was_critical: bool) {
        let mut acct = self.accounting.lock().expect("accounting lock poisoned");
        if was_critical {
            acct.active_critical = acct.active_critical.saturating_sub(1);
        } else {
            acct.active_standard = acct.active_standard.saturating_sub(1);
        }
    }

    /// Check whether the critical queue has capacity.
    pub fn critical_has_capacity(&self) -> bool {
        let state = self.queue.state.lock().expect("queue lock poisoned");
        state.critical.len() < self.queue.critical_capacity
    }

    /// Current queue metrics snapshot.
    pub fn metrics(&self) -> QueueMetrics {
        self.queue.metrics()
    }

    /// Shut down: drain remaining work, then workers get `None`.
    pub fn shutdown(&self) {
        self.queue.shutdown();
    }

    /// Active worker counts.
    pub fn active_workers(&self) -> (usize, usize) {
        let acct = self.accounting.lock().expect("accounting lock poisoned");
        (acct.active_critical, acct.active_standard)
    }

    /// Access the underlying queue (for blocking dequeue in worker threads).
    pub fn queue(&self) -> &WeightedFairQueue {
        &self.queue
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn push_ring(buf: &mut VecDeque<u64>, value: u64, cap: usize) {
    if buf.len() >= cap {
        buf.pop_front();
    }
    buf.push_back(value);
}

fn percentile(buf: &VecDeque<u64>, pct: u8) -> u64 {
    if buf.is_empty() {
        return 0;
    }
    let mut sorted: Vec<u64> = buf.iter().copied().collect();
    sorted.sort_unstable();
    let idx = ((pct as usize) * sorted.len() / 100).min(sorted.len() - 1);
    sorted[idx]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Classification tests ------------------------------------------------

    #[test]
    fn explicit_flag_overrides_everything() {
        assert!(is_critical_path(Some(true), "unknown_route", false));
        assert!(!is_critical_path(Some(false), "direct", true));
    }

    #[test]
    fn parent_inheritance_when_no_explicit_flag() {
        assert!(is_critical_path(None, "something_async", true));
        assert!(!is_critical_path(None, "something_async", false));
    }

    #[test]
    fn sync_routes_are_critical_by_default() {
        for route in &["direct", "council_only", "both", "memory_only", "graph_only"] {
            assert!(
                is_critical_path(None, route, false),
                "route '{route}' should be critical"
            );
        }
    }

    #[test]
    fn unknown_route_not_critical_without_parent() {
        assert!(!is_critical_path(None, "background_job", false));
    }

    #[test]
    fn inherit_sync_from_critical_parent() {
        assert!(inherit_critical_path(true, true));
    }

    #[test]
    fn async_never_inherits() {
        assert!(!inherit_critical_path(true, false));
        assert!(!inherit_critical_path(false, false));
    }

    #[test]
    fn sync_from_non_critical_parent_stays_false() {
        assert!(!inherit_critical_path(false, true));
    }

    // -- Missing flag defaults to false (backward compat) --------------------

    #[test]
    fn missing_flag_defaults_false_for_unknown_route() {
        assert!(!is_critical_path(None, "", false));
    }

    // -- Weighted Fair Queue tests -------------------------------------------

    #[test]
    fn empty_queue_try_dequeue_returns_none() {
        let q = WeightedFairQueue::with_defaults();
        assert!(q.try_dequeue().is_none());
    }

    #[test]
    fn single_critical_item_dequeues() {
        let q = WeightedFairQueue::with_defaults();
        q.enqueue(TaskItem::new("c1", true));
        let (item, event) = q.try_dequeue().unwrap();
        assert_eq!(item.id, "c1");
        assert!(item.critical_path);
        assert_eq!(event.priority, "critical");
    }

    #[test]
    fn single_standard_item_dequeues() {
        let q = WeightedFairQueue::with_defaults();
        q.enqueue(TaskItem::new("s1", false));
        let (item, _) = q.try_dequeue().unwrap();
        assert_eq!(item.id, "s1");
        assert!(!item.critical_path);
    }

    #[test]
    fn weighted_ratio_8_to_1() {
        let q = WeightedFairQueue::new(128);

        // Enqueue 10 critical + 10 standard
        for i in 0..10 {
            q.enqueue(TaskItem::new(format!("c{i}"), true));
            q.enqueue(TaskItem::new(format!("s{i}"), false));
        }

        // Dequeue 9 items (one full round: 8 critical + 1 standard)
        let mut critical_count = 0;
        let mut standard_count = 0;
        for _ in 0..9 {
            let (item, _) = q.try_dequeue().unwrap();
            if item.critical_path {
                critical_count += 1;
            } else {
                standard_count += 1;
            }
        }
        assert_eq!(critical_count, 8, "first 9 dequeues should yield 8 critical");
        assert_eq!(standard_count, 1, "first 9 dequeues should yield 1 standard");
    }

    #[test]
    fn fallthrough_when_preferred_lane_empty() {
        let q = WeightedFairQueue::new(128);

        // Only standard items — critical lane is empty.
        for i in 0..5 {
            q.enqueue(TaskItem::new(format!("s{i}"), false));
        }

        // Should still dequeue standard items even though round prefers critical.
        for i in 0..5 {
            let (item, _) = q.try_dequeue().unwrap();
            assert_eq!(item.id, format!("s{i}"));
        }
        assert!(q.try_dequeue().is_none());
    }

    #[test]
    fn no_starvation_of_standard_queue() {
        // Even with continuous critical arrivals, standard items must eventually
        // get served — specifically every 9th dequeue.
        let q = WeightedFairQueue::new(256);

        // Pre-fill: 100 critical + 1 standard
        for i in 0..100 {
            q.enqueue(TaskItem::new(format!("c{i}"), true));
        }
        q.enqueue(TaskItem::new("s0", false));

        let mut standard_seen = false;
        for _ in 0..101 {
            if let Some((item, _)) = q.try_dequeue() {
                if !item.critical_path {
                    standard_seen = true;
                    break;
                }
            }
        }
        assert!(standard_seen, "standard item must eventually be dequeued");
    }

    // -- Back-pressure tests -------------------------------------------------

    #[test]
    fn backpressure_rejects_when_critical_full() {
        let q = WeightedFairQueue::new(2);

        assert_eq!(q.enqueue(TaskItem::new("c1", true)), EnqueueResult::Accepted);
        assert_eq!(q.enqueue(TaskItem::new("c2", true)), EnqueueResult::Accepted);
        assert_eq!(
            q.enqueue(TaskItem::new("c3", true)),
            EnqueueResult::BackpressureCritical
        );

        // Standard items are unbounded
        assert_eq!(q.enqueue(TaskItem::new("s1", false)), EnqueueResult::Accepted);
    }

    #[test]
    fn backpressure_counter_increments() {
        let q = WeightedFairQueue::new(1);
        q.enqueue(TaskItem::new("c1", true));
        q.enqueue(TaskItem::new("c2", true)); // rejected
        q.enqueue(TaskItem::new("c3", true)); // rejected

        let m = q.metrics();
        assert_eq!(m.total_critical_rejected, 2);
    }

    // -- Metrics tests -------------------------------------------------------

    #[test]
    fn metrics_reflect_queue_state() {
        let q = WeightedFairQueue::new(64);
        q.enqueue(TaskItem::new("c1", true));
        q.enqueue(TaskItem::new("c2", true));
        q.enqueue(TaskItem::new("s1", false));

        let m = q.metrics();
        assert_eq!(m.critical_depth, 2);
        assert_eq!(m.standard_depth, 1);
        assert_eq!(m.total_critical_enqueued, 2);
        assert_eq!(m.total_standard_enqueued, 1);
    }

    #[test]
    fn metrics_update_after_dequeue() {
        let q = WeightedFairQueue::new(64);
        q.enqueue(TaskItem::new("c1", true));
        q.enqueue(TaskItem::new("s1", false));
        q.try_dequeue(); // dequeue c1

        let m = q.metrics();
        assert_eq!(m.total_critical_dequeued, 1);
        assert_eq!(m.critical_depth, 0);
    }

    // -- Dispatcher tests ----------------------------------------------------

    #[test]
    fn dispatcher_submit_and_acquire() {
        let d = Dispatcher::with_defaults();
        d.submit(TaskItem::new("t1", true));
        let (item, _) = d.acquire().unwrap();
        assert_eq!(item.id, "t1");
        d.release(true);
    }

    #[test]
    fn reserved_slot_blocks_standard_when_pool_almost_full() {
        // Config: 2 total workers, 1 reserved for critical.
        // That means standard can use at most 1 slot.
        let d = Dispatcher::new(DispatcherConfig {
            total_workers: 2,
            reserved_critical_slots: 1,
            critical_queue_capacity: 64,
        });

        // Fill the 1 standard slot.
        d.submit(TaskItem::new("s1", false));
        let (s1, _) = d.acquire().unwrap();
        assert!(!s1.critical_path);

        // Second standard item cannot acquire — reserved slot is off-limits.
        d.submit(TaskItem::new("s2", false));
        assert!(
            d.acquire().is_none(),
            "standard should not take reserved critical slot"
        );

        // But a critical item CAN use the reserved slot.
        d.submit(TaskItem::new("c1", true));
        let (c1, _) = d.acquire().unwrap();
        assert!(c1.critical_path);

        d.release(false); // release s1
        d.release(true); // release c1
    }

    #[test]
    fn dispatcher_active_worker_counts() {
        let d = Dispatcher::with_defaults();
        d.submit(TaskItem::new("c1", true));
        d.submit(TaskItem::new("s1", false));
        d.acquire().unwrap();
        d.acquire().unwrap();

        let (crit, std) = d.active_workers();
        assert_eq!(crit, 1);
        assert_eq!(std, 1);

        d.release(true);
        d.release(false);

        let (crit, std) = d.active_workers();
        assert_eq!(crit, 0);
        assert_eq!(std, 0);
    }

    #[test]
    fn critical_has_capacity_reflects_queue() {
        let d = Dispatcher::new(DispatcherConfig {
            total_workers: 4,
            reserved_critical_slots: 1,
            critical_queue_capacity: 2,
        });
        assert!(d.critical_has_capacity());
        d.submit(TaskItem::new("c1", true));
        d.submit(TaskItem::new("c2", true));
        assert!(!d.critical_has_capacity());
    }

    // -- Adversarial flood test ----------------------------------------------

    #[test]
    fn adversarial_flood_critical_queue_bounded() {
        // Simulate an attacker flooding the critical queue.
        // Verify: queue is bounded, rejected count matches, and standard
        // items still get served.
        let cap = 8;
        let q = WeightedFairQueue::new(cap);

        // Flood critical queue
        let mut accepted = 0u64;
        let mut rejected = 0u64;
        for i in 0..1000 {
            match q.enqueue(TaskItem::new(format!("flood-{i}"), true)) {
                EnqueueResult::Accepted => accepted += 1,
                EnqueueResult::BackpressureCritical => rejected += 1,
            }
        }

        assert_eq!(accepted, cap as u64);
        assert_eq!(rejected, 1000 - cap as u64);

        // Critical queue depth is bounded.
        let m = q.metrics();
        assert_eq!(m.critical_depth, cap);

        // Standard items still enqueue and dequeue normally.
        q.enqueue(TaskItem::new("legit-standard", false));
        assert_eq!(q.metrics().standard_depth, 1);

        // Drain a critical item, enqueue one more — should succeed now.
        let (item, _) = q.try_dequeue().unwrap();
        assert!(item.critical_path);
        assert_eq!(
            q.enqueue(TaskItem::new("flood-late", true)),
            EnqueueResult::Accepted
        );
    }

    #[test]
    fn adversarial_flood_standard_still_served() {
        // Even under heavy critical load, standard items must be reachable.
        let q = WeightedFairQueue::new(64);

        // Enqueue 64 critical + 1 standard
        for i in 0..64 {
            q.enqueue(TaskItem::new(format!("c{i}"), true));
        }
        q.enqueue(TaskItem::new("victim", false));

        // The standard item must appear within 65 dequeues (worst case:
        // 8 critical per round × 7 full rounds = 56, then standard slot at pos 8).
        let mut found = false;
        for _ in 0..65 {
            if let Some((item, _)) = q.try_dequeue() {
                if item.id == "victim" {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "standard item must be served even under heavy critical load");
    }

    #[test]
    fn adversarial_flood_dispatcher_reservation_holds() {
        // Under flood: standard tasks must still acquire slots because the
        // reserved critical slot doesn't let standard overflow steal it.
        let d = Dispatcher::new(DispatcherConfig {
            total_workers: 3,
            reserved_critical_slots: 1,
            critical_queue_capacity: 16,
        });

        // Fill 2 critical worker slots (3 total - 1 reserved = 2 unreserved,
        // but critical CAN use all 3).
        d.submit(TaskItem::new("c1", true));
        d.submit(TaskItem::new("c2", true));
        d.acquire().unwrap();
        d.acquire().unwrap();

        // Now 2 of 3 workers active (both critical).
        // A standard item should still be able to use the remaining slot
        // because it's not the reserved-for-critical slot that's busy.
        d.submit(TaskItem::new("s1", false));
        let result = d.acquire();
        assert!(
            result.is_some(),
            "standard should acquire when unreserved slots remain"
        );

        d.release(true);
        d.release(true);
        d.release(false);
    }

    // -- Shutdown test -------------------------------------------------------

    #[test]
    fn shutdown_drains_then_returns_none() {
        let q = WeightedFairQueue::new(64);
        q.enqueue(TaskItem::new("c1", true));
        q.enqueue(TaskItem::new("s1", false));
        q.shutdown();

        // Should still drain remaining items.
        assert!(q.try_dequeue().is_some());
        assert!(q.try_dequeue().is_some());
        assert!(q.try_dequeue().is_none());
    }

    // -- Wait-time observability test ----------------------------------------

    #[test]
    fn wait_time_recorded_in_metrics() {
        let q = WeightedFairQueue::new(64);
        q.enqueue(TaskItem::new("c1", true));
        // Small sleep to ensure non-zero wait.
        std::thread::sleep(std::time::Duration::from_millis(5));
        q.try_dequeue().unwrap();

        let m = q.metrics();
        assert!(m.critical_wait_ms_p50 >= 1, "wait time should be recorded");
    }
}
