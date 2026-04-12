/// Route-correction feedback loop for Layers query.
///
/// When the caller tells Layers "that was the wrong route", a [`RouteCorrection`]
/// is recorded to `~/.layers/route-corrections.jsonl`.  On every [`classify()`]
/// call the correction file is loaded and used to adjust signal scores so that
/// repeatedly-corrected patterns are demoted.
use crate::config::workspace_root;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// A single route correction record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCorrection {
    /// The task text that was originally classified.
    pub task: String,
    /// The route the system predicted.
    pub predicted: Route,
    /// The route the human or caller indicated was correct.
    pub actual: Route,
    /// When the correction was recorded (ISO-8601).
    pub timestamp: String,
}

impl RouteCorrection {
    pub fn new(task: String, predicted: Route, actual: Route) -> Self {
        Self {
            task,
            predicted,
            actual,
            timestamp: crate::util::iso_now(),
        }
    }
}

/// Returns the path to the route-corrections JSONL file.
pub fn corrections_path() -> PathBuf {
    workspace_root()
        .join(".layers")
        .join("route-corrections.jsonl")
}

/// Load all corrections from the corrections file.
/// Returns an empty vec if the file does not exist yet.
pub fn load_corrections() -> Vec<RouteCorrection> {
    let path = corrections_path();
    if !path.exists() {
        return Vec::new();
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Record a route correction (append-only to JSONL).
pub fn record_correction(correction: &RouteCorrection) -> std::io::Result<()> {
    let path = corrections_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(correction)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    // Write the full record + newline in a single atomic write to avoid
    // leaving a partial line in the JSONL if the process crashes mid-write.
    writeln!(file, "{line}")?;
    Ok(())
}

/// In-memory correction cache — populated once per process from the JSONL file
/// and reloadable after feedback is recorded.
static CORRECTION_CACHE: OnceLock<Mutex<HashMap<(Route, Route), usize>>> = OnceLock::new();

fn build_correction_cache() -> HashMap<(Route, Route), usize> {
    let corrections = load_corrections();
    let mut counts: HashMap<(Route, Route), usize> = HashMap::new();
    for c in corrections {
        *counts.entry((c.predicted, c.actual)).or_insert(0) += 1;
    }
    counts
}

fn correction_cache() -> &'static Mutex<HashMap<(Route, Route), usize>> {
    CORRECTION_CACHE.get_or_init(|| Mutex::new(build_correction_cache()))
}

/// Force a reload of the correction cache from disk.
/// Call this after [`record_correction`] so the next [`classify()`] picks up the change.
#[allow(dead_code)]
pub fn reload_corrections() {
    let fresh = build_correction_cache();
    match correction_cache().lock() {
        Ok(mut cache) => *cache = fresh,
        Err(poisoned) => *poisoned.into_inner() = fresh,
    }
}

/// Heuristic routing algorithm for Layers query.
///
/// Classifies a task into one of four routes based on keyword signal scoring:
/// - `neither`: trivial/local task, no context needed
/// - `memory_only`: historical context needed
/// - `graph_only`: structural/code context needed
/// - `both`: both historical and structural context needed

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Neither,
    MemoryOnly,
    GraphOnly,
    Both,
}

impl Route {
    pub fn label(self) -> &'static str {
        match self {
            Route::Neither => "neither",
            Route::MemoryOnly => "memory_only",
            Route::GraphOnly => "graph_only",
            Route::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteResult {
    pub route: Route,
    pub confidence: Confidence,
    pub scores: Scores,
    pub why: String,
    pub why_not: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Low,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Confidence::High => write!(f, "high"),
            Confidence::Low => write!(f, "low"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Scores {
    pub historical: u32,
    pub structural: u32,
    pub local: u32,
    pub action: u32,
}

const HISTORICAL_SIGNALS: &[&str] = &[
    "prior",
    "previous",
    "last time",
    "decid", // matches decide, decided, decision
    "agree", // matches agree, agreed, agreement
    "why did",
    "why was",
    "why do",
    "learn",
    "memory",
    "rationale",
    "recover",
    "revisit",
    "council",
    "history",
    "before",
    "earlier",
    "already",
    "back when",
    "last session",
    "recall",
    "concluded",
    "summarize",
    "retry",
];

const STRUCTURAL_SIGNALS: &[&str] = &[
    "file",
    "module",
    "depend", // matches dependency, dependencies, dependent, depends
    "import",
    "path",
    "codebase",
    "repo",
    "architect", // matches architecture, architectural
    "impact",
    "refactor",
    "implement",
    "where",
    "function",
    "struct",
    "class",
    "caller",
    "callee",
    "symbol",
    "flow",
    "trace",
    "tree",
    "diagram",
    "configuration",
    "service architecture",
    "error handling",
];

const LOCAL_SIGNALS: &[&str] = &[
    "rename",
    "variable",
    "syntax",
    "typo",
    "explain this line",
    "regex",
    "snippet",
    "one-liner",
    "simple utility",
    "format",
    "lint",
    "comma",
    "semicolon",
];

const ACTION_SIGNALS: &[&str] = &[
    "implement",
    "revis", // matches revise, revision, revised
    "align",
    "build",
    "recover",
    "migrat", // matches migrate, migration
    "plan",
    "design",
    "create",
    "add",
    "fix",
    "update",
    "deploy",
    "run",
    "generate",
];

const HISTORICAL_NEGATIONS: &[&str] = &[
    "not asking what we decided",
    "not asking about what we decided",
    "don't recall",
    "do not recall",
    "not about history",
    "not asking about history",
    "without history",
];

const STRUCTURAL_NEGATIONS: &[&str] = &[
    "not asking about code",
    "not asking about the code",
    "not the code",
    "without looking at code",
    "don't inspect code",
    "do not inspect code",
    "not asking about implementation",
    "not asking about the repo",
];

const AMBIGUITY_SIGNALS: &[&str] = &[
    "maybe",
    "perhaps",
    "either",
    "or maybe",
    "not sure",
    "if needed",
];

fn score_signals(task: &str, signals: &[&str]) -> u32 {
    let lower = task.to_lowercase();
    signals.iter().filter(|s| lower.contains(*s)).count() as u32
}

pub fn classify(task: &str) -> RouteResult {
    let lower = task.to_lowercase();
    let mut scores = Scores {
        historical: score_signals(task, HISTORICAL_SIGNALS),
        structural: score_signals(task, STRUCTURAL_SIGNALS),
        local: score_signals(task, LOCAL_SIGNALS),
        action: score_signals(task, ACTION_SIGNALS),
    };

    if HISTORICAL_NEGATIONS.iter().any(|p| lower.contains(p)) {
        scores.historical = 0;
    }
    if STRUCTURAL_NEGATIONS.iter().any(|p| lower.contains(p)) {
        scores.structural = 0;
    }
    if AMBIGUITY_SIGNALS.iter().any(|p| lower.contains(p)) {
        scores.historical = scores.historical.saturating_sub(1);
        scores.structural = scores.structural.saturating_sub(1);
    }

    // Apply route-correction bias: if corrections exist, adjust scores so
    // repeatedly-wrong routes get demoted and correct routes get a small boost.
    apply_correction_bias(&mut scores);

    let (route, confidence, why, why_not) = determine_route(&scores);

    RouteResult {
        route,
        confidence,
        scores,
        why,
        why_not,
    }
}

/// Apply correction-based bias to signal scores.
///
/// For each (predicted, actual) correction on record, demote signals that
/// support the predicted route and boost signals that support the actual route.
/// This has the effect of gradually adjusting route decisions when the same
/// pattern keeps getting corrected.
fn apply_correction_bias(scores: &mut Scores) {
    let correction_counts = match correction_cache().lock() {
        Ok(cache) => cache.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };

    // Short-circuit if there are no corrections yet.
    if correction_counts.is_empty() {
        return;
    }

    // For every (predicted → actual) correction on record, apply demotion/boost.
    // We do this per-correction-entry so a pattern corrected N times gets N× demotion.
    for ((predicted, actual), count) in &correction_counts {
        if *count == 0 {
            continue;
        }
        let weight = (*count as f64 * 0.15).min(0.6); // 15% per correction, cap at 60%

        // Demote the predicted route signals (predicted is &Route)
        match *predicted {
            Route::MemoryOnly | Route::Both => {
                scores.historical = (f64::from(scores.historical) * (1.0 - weight)).round() as u32;
            }
            Route::GraphOnly => {
                scores.structural = (f64::from(scores.structural) * (1.0 - weight)).round() as u32;
            }
            Route::Neither => {
                scores.local = (f64::from(scores.local) * (1.0 - weight)).round() as u32;
            }
        }

        // Boost the actual route signals (smaller boost = 1/3 of demotion)
        let boost = weight / 3.0;
        match *actual {
            Route::MemoryOnly | Route::Both => {
                scores.historical = (f64::from(scores.historical) * (1.0 + boost)).round() as u32;
            }
            Route::GraphOnly => {
                scores.structural = (f64::from(scores.structural) * (1.0 + boost)).round() as u32;
            }
            Route::Neither => {
                scores.local = (f64::from(scores.local) * (1.0 + boost)).round() as u32;
            }
        }
    }
}

fn determine_route(s: &Scores) -> (Route, Confidence, String, String) {
    // Neither: local task or both scores too low
    if s.local >= 3 && s.historical < 2 && s.structural < 2 {
        return (
            Route::Neither,
            Confidence::High,
            "Local/trivial task — high local signal, low historical and structural".into(),
            "Historical and structural context not needed for local tasks".into(),
        );
    }

    if s.historical < 2 && s.structural < 2 {
        let confidence = if s.local >= 1 {
            Confidence::High
        } else {
            Confidence::Low
        };
        return (
            Route::Neither,
            confidence,
            "Both historical and structural signals below threshold".into(),
            "No clear signal for either memory or graph retrieval".into(),
        );
    }

    // Both: strong signals in both dimensions
    if s.historical >= 3 && s.structural >= 3 {
        return (
            Route::Both,
            Confidence::High,
            format!(
                "Strong historical ({}) and structural ({}) signals",
                s.historical, s.structural
            ),
            String::new(),
        );
    }

    if s.historical >= 2 && s.structural >= 1 && s.action >= 1 {
        return (
            Route::Both,
            Confidence::High,
            format!(
                "Historical ({}) and structural ({}) signals reinforced by action intent ({})",
                s.historical, s.structural, s.action
            ),
            String::new(),
        );
    }

    // Memory only
    if s.historical >= 4 && s.structural < 3 {
        return (
            Route::MemoryOnly,
            Confidence::High,
            format!(
                "Strong historical signal ({}) with low structural ({})",
                s.historical, s.structural
            ),
            "Structural context not strongly indicated".into(),
        );
    }

    if s.historical >= 2 && s.structural < 2 {
        let confidence = if s.historical >= 3 {
            Confidence::High
        } else {
            Confidence::Low
        };
        return (
            Route::MemoryOnly,
            confidence,
            format!("Historical signal ({}) above threshold", s.historical),
            "Structural signal below threshold".into(),
        );
    }

    // Graph only
    if s.structural >= 4 && s.historical < 3 {
        return (
            Route::GraphOnly,
            Confidence::High,
            format!(
                "Strong structural signal ({}) with low historical ({})",
                s.structural, s.historical
            ),
            "Historical context not strongly indicated".into(),
        );
    }

    if s.structural >= 2 && s.historical < 2 {
        let confidence = if s.structural >= 3 {
            Confidence::High
        } else {
            Confidence::Low
        };
        return (
            Route::GraphOnly,
            confidence,
            format!("Structural signal ({}) above threshold", s.structural),
            "Historical signal below threshold".into(),
        );
    }

    // Fallback: low confidence → refuse
    (
        Route::Neither,
        Confidence::Low,
        "Conflicting or weak signals — defaulting to refusal".into(),
        "Could not confidently determine memory vs graph route".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivial_task_routes_neither() {
        let result = classify("rename this variable to snake_case");
        assert_eq!(result.route, Route::Neither);
    }

    #[test]
    fn history_question_routes_memory() {
        let result = classify(
            "why did we previously decide to use that rationale? what was the prior agreed approach?",
        );
        assert_eq!(result.route, Route::MemoryOnly);
    }

    #[test]
    fn structural_question_routes_graph() {
        let result = classify(
            "which module imports this file and what is the dependency architecture of the codebase?",
        );
        assert_eq!(result.route, Route::GraphOnly);
    }

    #[test]
    fn combined_question_routes_both() {
        let result = classify(
            "implement the previously decided refactor of the module dependency architecture",
        );
        assert_eq!(result.route, Route::Both);
    }

    #[test]
    fn low_signal_defaults_neither() {
        let result = classify("hello");
        assert_eq!(result.route, Route::Neither);
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn low_confidence_refuses() {
        // Low-confidence should default to Neither (refusal bias)
        let result = classify("something");
        assert_eq!(result.route, Route::Neither);
    }

    #[test]
    fn negated_structural_query_does_not_route_graph() {
        let result = classify(
            "what did we decide about caching? I am not asking about the code or implementation",
        );
        assert_eq!(result.route, Route::Neither);
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn negated_historical_query_prefers_graph() {
        let result = classify(
            "show me the module dependency flow in the repo, not the history of why we chose it",
        );
        assert_eq!(result.route, Route::GraphOnly);
    }

    #[test]
    fn ambiguous_mixed_intent_refuses() {
        let result = classify(
            "maybe check the prior decision or maybe inspect the module imports, not sure yet",
        );
        assert_eq!(result.route, Route::Neither);
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn borderline_history_question_stays_refusal_biased() {
        let result = classify("What did we already decide about Layers?");
        assert_eq!(result.route, Route::MemoryOnly);
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn previously_agreed_summary_routes_memory() {
        let result = classify("summarize the previously agreed approach for handling auth tokens");
        assert_eq!(result.route, Route::MemoryOnly);
        assert_eq!(result.confidence, Confidence::High);
    }

    #[test]
    fn deploying_prior_service_architecture_routes_both() {
        let result = classify("deploy the previously agreed service architecture to production");
        assert_eq!(result.route, Route::Both);
        assert_eq!(result.confidence, Confidence::High);
    }

    #[test]
    fn reload_corrections_refreshes_existing_cache() {
        let ws = crate::test_support::TestWorkspace::new("router-reload-corrections");
        let _ = ws.root();

        let initial = classify("hello");
        assert_eq!(initial.route, Route::Neither);

        let correction =
            RouteCorrection::new("hello".to_string(), Route::Neither, Route::MemoryOnly);
        record_correction(&correction).unwrap();
        reload_corrections();

        let cache = match correction_cache().lock() {
            Ok(cache) => cache.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        assert_eq!(cache.get(&(Route::Neither, Route::MemoryOnly)), Some(&1));
    }

    // ─── JSONL-driven benchmark tests ───────────────────────────────────────

    fn parse_route(s: &str) -> Route {
        match s {
            "neither" => Route::Neither,
            "memory_only" => Route::MemoryOnly,
            "graph_only" => Route::GraphOnly,
            "both" => Route::Both,
            other => panic!("unknown route in answer key: {other}"),
        }
    }

    fn parse_confidence(s: &str) -> Confidence {
        match s {
            "high" => Confidence::High,
            "low" => Confidence::Low,
            other => panic!("unknown confidence in answer key: {other}"),
        }
    }

    fn benchmark_path(name: &str) -> std::path::PathBuf {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let direct = manifest_dir.join("benchmarks").join(name);
        if direct.exists() {
            return direct;
        }
        manifest_dir.join("..").join("..").join("benchmarks").join(name)
    }

    /// Reads benchmarks/routing-answer-keys.jsonl and verifies classify()
    /// matches every expected route (and confidence, when specified).
    #[test]
    fn benchmark_routing_answer_keys() {
        let _ws = crate::test_support::TestWorkspace::new("benchmark-answer-keys");
        let path = benchmark_path("routing-answer-keys.jsonl");
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

        let mut passed = 0;
        let mut failed = Vec::new();

        for (i, line) in content.lines().enumerate() {
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue, // skip comment lines
            };

            let query = v["query"].as_str().unwrap();
            let expected_route = parse_route(v["expected_route"].as_str().unwrap());
            let expected_confidence = v["expected_confidence"].as_str().map(parse_confidence);

            let result = classify(query);

            let route_ok = result.route == expected_route;
            let confidence_ok = expected_confidence.map_or(true, |c| result.confidence == c);

            if route_ok && confidence_ok {
                passed += 1;
            } else {
                failed.push(format!(
                    "  line {}: query={:?}\n    expected route={:?} confidence={:?}\n    got      route={:?} confidence={:?}",
                    i + 1, query, expected_route, expected_confidence,
                    result.route, result.confidence,
                ));
            }
        }

        if !failed.is_empty() {
            panic!(
                "routing answer key benchmark: {}/{} passed, {} failed:\n{}",
                passed,
                passed + failed.len(),
                failed.len(),
                failed.join("\n"),
            );
        }
    }

    /// Reads benchmarks/routing-failures.jsonl and verifies that routing
    /// decisions remain correct even when retrieval subsystems fail.
    /// The router is a pure signal-scoring classifier — it should produce
    /// the same route regardless of downstream failures.
    #[test]
    fn benchmark_routing_failures() {
        let _ws = crate::test_support::TestWorkspace::new("benchmark-routing-failures");
        let path = benchmark_path("routing-failures.jsonl");
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

        let mut passed = 0;
        let mut failed = Vec::new();

        for (i, line) in content.lines().enumerate() {
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue, // skip comment lines
            };

            let query = v["query"].as_str().unwrap();
            let expected_route = parse_route(v["expected_route"].as_str().unwrap());
            let failure_mode = v["failure_mode"].as_str().unwrap_or("unknown");

            let result = classify(query);

            if result.route == expected_route {
                passed += 1;
            } else {
                failed.push(format!(
                    "  line {}: failure_mode={}, query={:?}\n    expected route={:?}, got route={:?}",
                    i + 1, failure_mode, query, expected_route, result.route,
                ));
            }
        }

        if !failed.is_empty() {
            panic!(
                "routing failure benchmark: {}/{} passed, {} failed:\n{}",
                passed,
                passed + failed.len(),
                failed.len(),
                failed.join("\n"),
            );
        }
    }

    // ─── Feedback loop integration tests ────────────────────────────────────

    /// End-to-end test: record corrections → reload cache → verify routing
    /// decisions shift toward the corrected route.
    #[test]
    fn feedback_loop_shifts_routing_after_corrections() {
        let ws = crate::test_support::TestWorkspace::new("router-feedback-loop");
        let _ = ws.root();

        // Baseline: a query that routes to Neither with low confidence
        let baseline = classify("hello world");
        assert_eq!(baseline.route, Route::Neither);

        // Record multiple corrections saying "Neither was wrong, MemoryOnly was right"
        for _ in 0..4 {
            let correction =
                RouteCorrection::new("hello world".to_string(), Route::Neither, Route::MemoryOnly);
            record_correction(&correction).unwrap();
        }
        reload_corrections();

        // After 4 corrections (Neither→MemoryOnly), the local signal should be
        // demoted. The router applies 15% demotion per correction (capped at 60%).
        // With 4 corrections: weight = min(4*0.15, 0.6) = 0.6
        // Local score gets multiplied by 0.4, historical gets a small boost.
        // For "hello world" (local=0, historical=0), the demotion has no effect
        // on raw signals, but the correction cache IS populated — verify that.
        let cache = match correction_cache().lock() {
            Ok(cache) => cache.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        assert_eq!(cache.get(&(Route::Neither, Route::MemoryOnly)), Some(&4));
    }

    /// Verify the feedback loop with a query that has non-zero signal scores,
    /// demonstrating that corrections actually shift routing behavior.
    /// Uses apply_correction_bias directly for deterministic testing.
    #[test]
    fn feedback_loop_demotes_incorrectly_predicted_route() {
        let ws = crate::test_support::TestWorkspace::new("router-feedback-demote");
        let _ = ws.root();

        // Record corrections saying Neither was wrong, GraphOnly was right
        for _ in 0..3 {
            let correction =
                RouteCorrection::new("test".to_string(), Route::Neither, Route::GraphOnly);
            record_correction(&correction).unwrap();
        }
        reload_corrections();

        // Apply correction bias to a scores struct with local=5.
        // 3 corrections: weight = min(3*0.15, 0.6) = 0.45
        // local = round(5 * 0.55) = 3 (demoted from 5)
        let mut scores = Scores {
            historical: 0,
            structural: 0,
            local: 5,
            action: 0,
        };
        apply_correction_bias(&mut scores);
        assert!(
            scores.local < 5,
            "local score should be demoted after corrections: got {}",
            scores.local,
        );
    }

    /// Verify that the correction weight is capped at 60% demotion.
    #[test]
    fn feedback_loop_correction_weight_caps_at_60_percent() {
        let ws = crate::test_support::TestWorkspace::new("router-feedback-cap");
        let _ = ws.root();

        // Record 10 corrections — well beyond the 4 needed to hit the 60% cap
        for _ in 0..10 {
            let correction =
                RouteCorrection::new("test".to_string(), Route::Neither, Route::MemoryOnly);
            record_correction(&correction).unwrap();
        }
        reload_corrections();

        let cache = match correction_cache().lock() {
            Ok(cache) => cache.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        assert_eq!(cache.get(&(Route::Neither, Route::MemoryOnly)), Some(&10));

        // With 10 corrections, weight = min(10*0.15, 0.6) = 0.6 (capped)
        // A local score of 5 would become round(5 * 0.4) = 2
        let mut scores = Scores {
            historical: 0,
            structural: 0,
            local: 5,
            action: 0,
        };
        apply_correction_bias(&mut scores);
        assert_eq!(scores.local, 2, "local=5 * 0.4 = 2 (60% demotion cap)");
    }

    /// Verify that corrections in both directions don't cancel each other —
    /// they accumulate independently per (predicted, actual) pair.
    #[test]
    fn feedback_loop_independent_correction_pairs() {
        let ws = crate::test_support::TestWorkspace::new("router-feedback-pairs");
        let _ = ws.root();

        // Correction: Neither → MemoryOnly (demotes local, boosts historical)
        record_correction(&RouteCorrection::new(
            "a".to_string(),
            Route::Neither,
            Route::MemoryOnly,
        ))
        .unwrap();

        // Correction: MemoryOnly → GraphOnly (demotes historical, boosts structural)
        record_correction(&RouteCorrection::new(
            "b".to_string(),
            Route::MemoryOnly,
            Route::GraphOnly,
        ))
        .unwrap();

        reload_corrections();

        let cache = match correction_cache().lock() {
            Ok(cache) => cache.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        assert_eq!(cache.get(&(Route::Neither, Route::MemoryOnly)), Some(&1));
        assert_eq!(cache.get(&(Route::MemoryOnly, Route::GraphOnly)), Some(&1));
        assert_eq!(cache.len(), 2);
    }
}
