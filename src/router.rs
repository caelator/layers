/// Route-correction feedback loop for Layers query.
///
/// When the caller tells Layers "that was the wrong route", a [`RouteCorrection`]
/// is recorded to `~/.layers/route-corrections.jsonl`.  On every [`classify()`]
/// call the correction file is loaded and used to adjust signal scores so that
/// repeatedly-corrected patterns are demoted.
use crate::config::workspace_root;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

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
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?
        .write_all(line.as_bytes())?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?
        .write_all(b"\n")?;
    Ok(())
}

/// In-memory correction cache — populated once per process from the JSONL file.
static CORRECTION_CACHE: std::sync::OnceLock<HashMap<(Route, Route), usize>> =
    std::sync::OnceLock::new();

fn load_correction_cache() -> &'static HashMap<(Route, Route), usize> {
    CORRECTION_CACHE.get_or_init(|| {
        let corrections = load_corrections();
        let mut counts: HashMap<(Route, Route), usize> = HashMap::new();
        for c in corrections {
            *counts.entry((c.predicted, c.actual)).or_insert(0) += 1;
        }
        counts
    })
}

/// Force a reload of the correction cache from disk.
/// Call this after [`record_correction`] so the next [`classify()`] picks up the change.
#[allow(dead_code)]
pub fn reload_corrections() {
    let _ = CORRECTION_CACHE.set({
        let corrections = load_corrections();
        let mut counts: HashMap<(Route, Route), usize> = HashMap::new();
        for c in corrections {
            *counts.entry((c.predicted, c.actual)).or_insert(0) += 1;
        }
        counts
    });
}

/// Heuristic routing algorithm for Layers query.
///
/// Classifies a task into one of four routes based on keyword signal scoring:
/// - `neither`: trivial/local task, no context needed
/// - `memory_only`: historical context needed
/// - `graph_only`: structural/code context needed
/// - `both`: both historical and structural context needed
use std::fmt;

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
    // Short-circuit if there are no corrections yet.
    if load_correction_cache().is_empty() {
        return;
    }

    // For every (predicted → actual) correction on record, apply demotion/boost.
    // We do this per-correction-entry so a pattern corrected N times gets N× demotion.
    for ((predicted, actual), &count) in load_correction_cache() {
        if count == 0 {
            continue;
        }
        let weight = (count as f64 * 0.15).min(0.6); // 15% per correction, cap at 60%

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

    if s.historical >= 2 && s.structural >= 2 && s.action >= 1 {
        return (
            Route::Both,
            Confidence::High,
            format!(
                "Moderate historical ({}) and structural ({}) signals with action intent ({})",
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
}
