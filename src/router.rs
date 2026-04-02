/// Heuristic routing algorithm for Layers query.
///
/// Classifies a task into one of four routes based on keyword signal scoring:
/// - `neither`: trivial/local task, no context needed
/// - `memory_only`: historical context needed
/// - `graph_only`: structural/code context needed
/// - `both`: both historical and structural context needed

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    "decid",     // matches decide, decided, decision
    "agree",     // matches agree, agreed, agreement
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
    "depend",    // matches dependency, dependencies, dependent, depends
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
    "revis",     // matches revise, revision, revised
    "align",
    "build",
    "recover",
    "migrat",    // matches migrate, migration
    "plan",
    "design",
    "create",
    "add",
    "fix",
    "update",
];

fn score_signals(task: &str, signals: &[&str]) -> u32 {
    let lower = task.to_lowercase();
    signals
        .iter()
        .filter(|s| lower.contains(&s.to_lowercase()))
        .count() as u32
}

pub fn classify(task: &str) -> RouteResult {
    let scores = Scores {
        historical: score_signals(task, HISTORICAL_SIGNALS),
        structural: score_signals(task, STRUCTURAL_SIGNALS),
        local: score_signals(task, LOCAL_SIGNALS),
        action: score_signals(task, ACTION_SIGNALS),
    };

    let (route, confidence, why, why_not) = determine_route(&scores);

    RouteResult {
        route,
        confidence,
        scores,
        why,
        why_not,
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
        let result = classify("why did we previously decide to use that rationale? what was the prior agreed approach?");
        assert_eq!(result.route, Route::MemoryOnly);
    }

    #[test]
    fn structural_question_routes_graph() {
        let result = classify("which module imports this file and what is the dependency architecture of the codebase?");
        assert_eq!(result.route, Route::GraphOnly);
    }

    #[test]
    fn combined_question_routes_both() {
        let result = classify("implement the previously decided refactor of the module dependency architecture");
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
}
