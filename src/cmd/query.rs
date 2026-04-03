use anyhow::Result;
use serde_json::json;

use crate::config::memoryport_dir;
use crate::memory;
use crate::graph;
use crate::router::{self, Confidence, Route};
use crate::util::{append_jsonl, iso_now};

const MAX_MEMORY_RECORDS: usize = 3;
const MAX_GITNEXUS_FACTS: usize = 5;
const MAX_OUTPUT_WORDS: usize = 1200;

pub fn handle_query(task: &str, json_out: bool, no_audit: bool) -> Result<()> {
    let route_result = router::classify(task);

    // Low-confidence downgrades to neither (refusal bias)
    let effective_route = if route_result.confidence == Confidence::Low {
        Route::Neither
    } else {
        route_result.route
    };

    let mut evidence_sections: Vec<String> = Vec::new();
    let mut open_uncertainty: Vec<String> = Vec::new();

    // Retrieve memory if routed
    if matches!(effective_route, Route::MemoryOnly | Route::Both) {
        match memory::retrieve_relevant(task, MAX_MEMORY_RECORDS) {
            Ok(records) if !records.is_empty() => {
                let lines: Vec<String> = records
                    .iter()
                    .map(|r| {
                        if r.timestamp.is_empty() {
                            format!("- [{}] {}", r.source, r.text)
                        } else {
                            format!("- [{}][{}] {}", r.source, r.timestamp, r.text)
                        }
                    })
                    .collect();
                evidence_sections.push(format!("### Memory\n{}", lines.join("\n")));
            }
            Ok(_) => {
                open_uncertainty.push("Memory retrieval returned no matching records.".into());
            }
            Err(e) => {
                open_uncertainty.push(format!("Memory retrieval failed: {e}"));
            }
        }
    }

    // Retrieve graph context if routed
    if matches!(effective_route, Route::GraphOnly | Route::Both) {
        match graph::query(task, MAX_GITNEXUS_FACTS) {
            Ok(facts) if !facts.is_empty() => {
                evidence_sections.push(format!("### GitNexus\n{}", facts.join("\n")));
            }
            Ok(_) => {
                open_uncertainty
                    .push("GitNexus query returned no results. Run `layers refresh` to update the index.".into());
            }
            Err(e) => {
                open_uncertainty.push(format!("GitNexus retrieval failed: {e}"));
            }
        }
    }

    // Enforce word budget
    let evidence_text = evidence_sections.join("\n\n");
    let word_count = evidence_text.split_whitespace().count();
    let (final_evidence, budget_exceeded) = if word_count > MAX_OUTPUT_WORDS {
        open_uncertainty.push(format!(
            "Evidence exceeded {MAX_OUTPUT_WORDS}-word budget ({word_count} words). Truncated."
        ));
        let truncated: String = evidence_text
            .split_whitespace()
            .take(MAX_OUTPUT_WORDS)
            .collect::<Vec<_>>()
            .join(" ");
        (truncated, true)
    } else {
        (evidence_text, false)
    };

    // Audit log (skip if --no-audit)
    if !no_audit {
        let audit = json!({
            "timestamp": iso_now(),
            "action": "query",
            "task": task,
            "route": route_result.route.label(),
            "effective_route": effective_route.label(),
            "confidence": route_result.confidence.to_string(),
            "scores": route_result.scores,
            "budget_exceeded": budget_exceeded,
            "evidence_words": word_count,
        });
        let audit_path = memoryport_dir().join("layers-audit.jsonl");
        append_jsonl(&audit_path, &audit)?;
    }

    if json_out {
        let output = json!({
            "route": effective_route.label(),
            "confidence": route_result.confidence.to_string(),
            "scores": route_result.scores,
            "why_retrieved": route_result.why,
            "why_not_retrieved": route_result.why_not,
            "evidence": final_evidence,
            "open_uncertainty": open_uncertainty,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if matches!(effective_route, Route::Neither) {
        println!("<layers_context>");
        println!("Route: {}", effective_route.label());
        println!("Why Not Retrieved: {}", route_result.why);
        println!("No context injection — task does not warrant retrieval.");
        println!("</layers_context>");
    } else {
        println!("<layers_context>");
        println!("Route: {}", effective_route.label());
        println!("Why Retrieved: {}", route_result.why);
        if !route_result.why_not.is_empty() {
            println!("Why Not Retrieved: {}", route_result.why_not);
        }
        if !final_evidence.is_empty() {
            println!("\nEvidence:");
            println!("{final_evidence}");
        }
        if !open_uncertainty.is_empty() {
            println!("\nOpen Uncertainty:");
            for u in &open_uncertainty {
                println!("- {u}");
            }
        }
        println!("</layers_context>");
    }

    Ok(())
}
