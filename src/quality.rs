//! Result quality evaluation for the Layers routing feedback loop.
//!
//! After retrieval, `evaluate()` scores the returned results against the
//! original query. Low scores produce `RouteFailure::Soft` records that
//! feed back into the route-weight system, closing the quality loop even
//! when retrieval "succeeds" but returns poor results.

use crate::feedback::{
    FailureKind, RouteFailure, RouteId, RoutingSignals, SoftErrorKind, emit_failure,
};

// ─── Thresholds ─────────────────────────────────────────────────────────────

/// Minimum fraction of query terms that must appear in at least one result.
const RELEVANCE_THRESHOLD: f64 = 0.25;

/// Minimum average words per result to avoid flagging as too thin.
const SPECIFICITY_MIN_WORDS: usize = 8;

// ─── Public API ─────────────────────────────────────────────────────────────

/// Quality dimensions for a set of retrieved results.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ResultQuality {
    /// Fraction of query terms found in at least one result (0.0–1.0).
    pub relevance: f64,
    /// Number of results returned vs. number requested (0.0–1.0).
    pub coverage: f64,
    /// Average word count per result.
    pub avg_words: f64,
    /// Whether quality is acceptable (all dimensions above thresholds).
    pub acceptable: bool,
    /// Human-readable reason if not acceptable.
    pub reason: Option<String>,
}

/// Evaluate the quality of retrieved results against the original query.
///
/// - `query`: the user's original query text
/// - `results`: the text content of each retrieved result
/// - `requested`: how many results were requested from the retriever
///
/// Returns a `ResultQuality` with per-dimension scores.
pub fn evaluate(query: &str, results: &[&str], requested: usize) -> ResultQuality {
    if results.is_empty() {
        return ResultQuality {
            relevance: 0.0,
            coverage: 0.0,
            avg_words: 0.0,
            acceptable: false,
            reason: Some("No results returned".into()),
        };
    }

    let relevance = compute_relevance(query, results);
    let coverage = if requested == 0 {
        1.0
    } else {
        (results.len() as f64 / requested as f64).min(1.0)
    };
    let avg_words = results.iter().map(|r| r.split_whitespace().count()).sum::<usize>() as f64
        / results.len() as f64;

    let mut reasons: Vec<&str> = Vec::new();
    if relevance < RELEVANCE_THRESHOLD {
        reasons.push("low relevance — query terms rarely appear in results");
    }
    if avg_words < SPECIFICITY_MIN_WORDS as f64 {
        reasons.push("low specificity — results are too short/generic");
    }

    let acceptable = reasons.is_empty();
    let reason = if reasons.is_empty() {
        None
    } else {
        Some(reasons.join("; "))
    };

    ResultQuality {
        relevance,
        coverage,
        avg_words,
        acceptable,
        reason,
    }
}

/// Emit a Soft failure if quality is unacceptable.
///
/// Returns `true` if a failure was emitted.
pub fn emit_if_poor(
    quality: &ResultQuality,
    query: &str,
    route_id: RouteId,
    stage: &str,
    signals: RoutingSignals,
) -> bool {
    if quality.acceptable {
        return false;
    }

    let _ = quality; // all current failure modes map to InsufficientContext

    let failure = RouteFailure::new(
        query.to_string(),
        route_id,
        FailureKind::Soft {
            error_kind: SoftErrorKind::InsufficientContext,
            flagged_by: "quality-evaluator".to_string(),
            affected_stage: stage.to_string(),
        },
        signals,
    )
    .with_note(quality.reason.clone().unwrap_or_default());

    if let Err(e) = emit_failure(&failure) {
        eprintln!("[route-feedback] failed to emit quality failure: {e}");
    }
    true
}

// ─── Internals ──────────────────────────────────────────────────────────────

/// Extract meaningful terms from the query (lowercase, deduplicated, stopwords removed).
fn query_terms(query: &str) -> Vec<String> {
    let stopwords: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has",
        "had", "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall",
        "can", "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "about",
        "it", "its", "this", "that", "these", "those", "i", "we", "you", "he", "she", "they",
        "me", "us", "him", "her", "them", "my", "our", "your", "his", "their", "and", "or",
        "but", "not", "no", "if", "then", "so", "what", "how", "when", "where", "why", "which",
        "who", "whom",
    ];

    let mut seen = std::collections::HashSet::new();
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2 && !stopwords.contains(w))
        .filter(|w| seen.insert((*w).to_string()))
        .map(std::string::ToString::to_string)
        .collect()
}

/// Fraction of query terms that appear as whole words in at least one result.
fn compute_relevance(query: &str, results: &[&str]) -> f64 {
    let terms = query_terms(query);
    if terms.is_empty() {
        return 1.0; // trivial query — can't judge relevance
    }

    // Tokenize all results into a word set for whole-word matching
    let combined: String = results.iter().map(|r| r.to_lowercase()).collect::<Vec<_>>().join(" ");
    let result_words: std::collections::HashSet<&str> = combined
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();

    let hits = terms.iter().filter(|t| result_words.contains(t.as_str())).count();
    hits as f64 / terms.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_results_are_unacceptable() {
        let q = evaluate("how does auth work", &[], 3);
        assert!(!q.acceptable);
        assert_eq!(q.relevance, 0.0);
        assert_eq!(q.coverage, 0.0);
    }

    #[test]
    fn relevant_results_pass() {
        let results = &[
            "The auth module validates JWT tokens before passing to middleware",
            "Auth flow starts in src/auth/handler.rs and calls validate_token()",
        ];
        let q = evaluate("how does auth validation work", results, 3);
        assert!(q.relevance > RELEVANCE_THRESHOLD);
        assert!(q.acceptable);
    }

    #[test]
    fn irrelevant_results_fail() {
        let results = &[
            "The database migration script runs on deploy",
            "CI pipeline configuration is in .github/workflows",
        ];
        let q = evaluate("how does auth validation work", results, 3);
        assert!(q.relevance < RELEVANCE_THRESHOLD);
        assert!(!q.acceptable);
    }

    #[test]
    fn short_results_flagged_as_low_specificity() {
        let results = &["auth ok", "token valid"];
        let q = evaluate("how does auth work", results, 3);
        // Results mention auth/token but are too short
        assert!(q.avg_words < SPECIFICITY_MIN_WORDS as f64);
        assert!(!q.acceptable);
    }

    #[test]
    fn coverage_reflects_fill_ratio() {
        let results = &["result one with enough words to pass specificity check easily"];
        let q = evaluate("test query terms", results, 5);
        assert!((q.coverage - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn trivial_query_gets_full_relevance() {
        // A query with only stopwords should not penalize results
        let results = &["some detailed result with many words for specificity check"];
        let q = evaluate("the is a", results, 1);
        assert_eq!(q.relevance, 1.0);
    }

    #[test]
    fn query_terms_deduplicates_and_filters_stopwords() {
        let terms = query_terms("the auth auth module is working");
        assert!(terms.contains(&"auth".to_string()));
        assert!(terms.contains(&"module".to_string()));
        assert!(terms.contains(&"working".to_string()));
        // "the" and "is" are stopwords
        assert!(!terms.contains(&"the".to_string()));
        assert!(!terms.contains(&"is".to_string()));
        // "auth" should appear only once
        assert_eq!(terms.iter().filter(|t| *t == "auth").count(), 1);
    }

    #[test]
    fn zero_requested_gives_full_coverage() {
        let results = &["some result with enough words for the specificity threshold"];
        let q = evaluate("test query", results, 0);
        assert_eq!(q.coverage, 1.0);
    }
}
