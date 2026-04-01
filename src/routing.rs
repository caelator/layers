use crate::types::RouteDecision;
use regex::Regex;
use serde_json::json;

fn score_patterns(text: &str, patterns: &[(&str, i32)]) -> (i32, Vec<String>) {
    let mut score = 0;
    let mut matched = Vec::new();
    for (pattern, weight) in patterns {
        let re = Regex::new(pattern).unwrap();
        if re.is_match(text) {
            score += weight;
            matched.push(pattern.to_string());
        }
    }
    (score, matched)
}

pub fn route_query(query: &str) -> RouteDecision {
    let lowered = query.to_lowercase();
    let historical_patterns = vec![
        (r"\bprevious\b", 2),
        (r"\bprior\b", 2),
        (r"\blast time\b", 3),
        (r"\bdecid(?:e|ed|ing)\b", 3),
        (r"\bagreed?\b", 2),
        (r"\bapproved\b", 3),
        (r"\bcouncil\b", 2),
        (r"\bredesign\b", 2),
        (r"\bpersona(?:s)?\b", 2),
        (r"\benforcement\b", 2),
        (r"\bpolicy\b", 2),
        (r"\bwhy\b", 1),
        (r"\blearn(?:ing|ed|ings)?\b", 2),
        (r"\bmemory\b", 2),
        (r"\brationale\b", 2),
        (r"\brecover\b", 2),
        (r"\brevisit\b", 2),
        (r"\bplan\b", 1),
        (r"\bstatus\b", 2),
        (r"\bnext step\b", 2),
    ];
    let structural_patterns = vec![
        (r"\bfile(?:s)?\b", 2),
        (r"\bmodule(?:s)?\b", 2),
        (r"\bdependenc(?:y|ies)\b", 3),
        (r"\bimport(?:s)?\b", 2),
        (r"\bpath\b", 2),
        (r"\bcodebase\b", 2),
        (r"\brepo(?:sitory)?\b", 2),
        (r"\barchitecture\b", 2),
        (r"\bimpact\b", 3),
        (r"\brefactor\b", 3),
        (r"\bimplement\b", 2),
        (r"\bwhere\b", 1),
    ];
    let local_patterns = vec![
        (r"\brename\b", 3),
        (r"\bvariable\b", 3),
        (r"\bsyntax\b", 3),
        (r"\btypo\b", 3),
        (r"\bexplain this line\b", 4),
        (r"\bregex\b", 3),
        (r"\bsnippet\b", 3),
        (r"\bone-liner\b", 3),
        (r"\bsimple utility\b", 3),
    ];
    let action_patterns = vec![
        (r"\bimplement\b", 2),
        (r"\brevise\b", 2),
        (r"\balign\b", 2),
        (r"\bbuild\b", 2),
        (r"\brecover\b", 2),
        (r"\bmigrate\b", 2),
    ];

    let (historical_score, historical_matches) = score_patterns(&lowered, &historical_patterns);
    let (structural_score, structural_matches) = score_patterns(&lowered, &structural_patterns);
    let (local_score, local_matches) = score_patterns(&lowered, &local_patterns);
    let (action_score, action_matches) = score_patterns(&lowered, &action_patterns);

    let (route, confidence, rationale) =
        if local_score >= 3 && historical_score < 3 && structural_score < 3 {
            (
                "neither".to_string(),
                "high".to_string(),
                vec!["Local-edit signal dominated; retrieval would likely add noise.".to_string()],
            )
        } else if historical_score < 2 && structural_score < 2 {
            (
                "neither".to_string(),
                "high".to_string(),
                vec!["Historical and structural need are both below threshold.".to_string()],
            )
        } else if historical_score >= 3 && structural_score >= 3 {
            (
                "both".to_string(),
                "high".to_string(),
                vec![
                    "Historical and structural scores both exceeded the dual-retrieval threshold."
                        .to_string(),
                ],
            )
        } else if historical_score >= 2 && structural_score >= 2 && action_score >= 1 {
            (
                "both".to_string(),
                "medium".to_string(),
                vec![
                    "Implementation/revision task needs both past decisions and repo structure."
                        .to_string(),
                ],
            )
        } else if historical_score >= 4 && structural_score < 3 {
            (
                "memory_only".to_string(),
                "high".to_string(),
                vec![
                "Historical score cleared the memory threshold without enough structural demand."
                    .to_string(),
            ],
            )
        } else if structural_score >= 4 && historical_score < 3 {
            (
                "graph_only".to_string(),
                "high".to_string(),
                vec![
                "Structural score cleared the graph threshold without enough historical demand."
                    .to_string(),
            ],
            )
        } else {
            (
                "neither".to_string(),
                "low".to_string(),
                vec![
                    "Signals are mixed or weak; refusal is safer than speculative retrieval."
                        .to_string(),
                ],
            )
        };

    let scores = serde_json::Map::from_iter(vec![
        ("historical".to_string(), json!(historical_score)),
        ("structural".to_string(), json!(structural_score)),
        ("local".to_string(), json!(local_score)),
        ("action".to_string(), json!(action_score)),
    ]);
    let matches = serde_json::Map::from_iter(vec![
        ("historical".to_string(), json!(historical_matches)),
        ("structural".to_string(), json!(structural_matches)),
        ("local".to_string(), json!(local_matches)),
        ("action".to_string(), json!(action_matches)),
    ]);

    RouteDecision {
        route,
        confidence,
        scores,
        matches,
        rationale,
    }
}
