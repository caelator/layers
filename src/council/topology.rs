/// Council topology — defines which stages run and in what order.
///
/// Different task profiles benefit from different stage sequences.  The
/// `select_topology` function picks the right one based on route, confidence,
/// and routing signals.
///
/// A single stage definition inside a topology.
#[derive(Debug, Clone)]
pub struct StageDef {
    /// Short identifier used in file names and records (e.g. "gemini").
    pub stage: &'static str,
    /// Human-friendly model label (e.g. "Gemini").
    pub model: &'static str,
    /// One-line description of what this stage does.
    pub role: &'static str,
}

/// An ordered sequence of stages that the council will execute.
#[derive(Debug, Clone)]
pub struct CouncilTopology {
    /// Machine-readable name persisted in `CouncilRunRecord.topology_name`.
    pub name: &'static str,
    /// Ordered list of stages to execute.
    pub stages: Vec<StageDef>,
}

// ── Built-in topology variants ──────────────────────────────────────────────

/// Gemini → Claude → Codex  (current default, for complex tasks).
pub fn full_council() -> CouncilTopology {
    CouncilTopology {
        name: "full_council",
        stages: vec![
            StageDef {
                stage: "gemini",
                model: "Gemini",
                role: "Generate options before convergence.",
            },
            StageDef {
                stage: "claude",
                model: "Claude",
                role: "Critique Gemini's draft and surface risks.",
            },
            StageDef {
                stage: "codex",
                model: "Codex",
                role: "Converge on the smallest reliable executable outcome.",
            },
        ],
    }
}

/// Claude → Codex  (high-confidence tasks with memory context).
pub fn fast_path() -> CouncilTopology {
    CouncilTopology {
        name: "fast_path",
        stages: vec![
            StageDef {
                stage: "claude",
                model: "Claude",
                role: "Critique and surface risks.",
            },
            StageDef {
                stage: "codex",
                model: "Codex",
                role: "Converge on the smallest reliable executable outcome.",
            },
        ],
    }
}

/// Codex only  (high-confidence tasks with no retrieval).
pub fn single_pass() -> CouncilTopology {
    CouncilTopology {
        name: "single_pass",
        stages: vec![StageDef {
            stage: "codex",
            model: "Codex",
            role: "Converge on the smallest reliable executable outcome.",
        }],
    }
}

// ── Topology selection ──────────────────────────────────────────────────────

/// Pick the right topology based on routing context.
///
/// * `route` – the resolved route string (e.g. "direct", `"memory_only"`, …).
/// * `confidence` – router confidence in `[0.0, 1.0]`.
/// * `has_memory_context` – whether memory/retrieval context is available.
pub fn select_topology(route: &str, confidence: f32, has_memory_context: bool) -> CouncilTopology {
    // High-confidence, no retrieval → single codex pass is sufficient.
    if confidence >= 0.9 && !has_memory_context && route == "direct" {
        return single_pass();
    }

    // High-confidence with memory context → skip Gemini exploratory stage.
    if confidence >= 0.8 && has_memory_context {
        return fast_path();
    }

    // Default: full three-stage council.
    full_council()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_council_has_three_stages() {
        let t = full_council();
        assert_eq!(t.name, "full_council");
        assert_eq!(t.stages.len(), 3);
        assert_eq!(t.stages[0].stage, "gemini");
        assert_eq!(t.stages[1].stage, "claude");
        assert_eq!(t.stages[2].stage, "codex");
    }

    #[test]
    fn fast_path_has_two_stages() {
        let t = fast_path();
        assert_eq!(t.name, "fast_path");
        assert_eq!(t.stages.len(), 2);
        assert_eq!(t.stages[0].stage, "claude");
        assert_eq!(t.stages[1].stage, "codex");
    }

    #[test]
    fn single_pass_has_one_stage() {
        let t = single_pass();
        assert_eq!(t.name, "single_pass");
        assert_eq!(t.stages.len(), 1);
        assert_eq!(t.stages[0].stage, "codex");
    }

    #[test]
    fn select_single_pass_high_confidence_no_memory_direct() {
        let t = select_topology("direct", 0.95, false);
        assert_eq!(t.name, "single_pass");
    }

    #[test]
    fn select_fast_path_high_confidence_with_memory() {
        let t = select_topology("memory_only", 0.85, true);
        assert_eq!(t.name, "fast_path");
    }

    #[test]
    fn select_full_council_low_confidence() {
        let t = select_topology("direct", 0.5, false);
        assert_eq!(t.name, "full_council");
    }

    #[test]
    fn select_full_council_high_confidence_non_direct_no_memory() {
        // High confidence but route is not "direct" and no memory → full council.
        let t = select_topology("graph_only", 0.95, false);
        assert_eq!(t.name, "full_council");
    }

    #[test]
    fn select_fast_path_threshold_boundary() {
        // Exactly 0.8 confidence with memory → fast_path.
        let t = select_topology("memory_only", 0.8, true);
        assert_eq!(t.name, "fast_path");
    }

    #[test]
    fn select_full_council_below_fast_path_threshold() {
        // 0.79 confidence with memory → not enough for fast_path.
        let t = select_topology("memory_only", 0.79, true);
        assert_eq!(t.name, "full_council");
    }
}
