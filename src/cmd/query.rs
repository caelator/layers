use anyhow::Result;
use serde_json::json;
use std::time::Instant;

use crate::config::{memoryport_dir, CONTEXT_PAYLOAD_SCHEMA_VERSION};
use crate::graph;
use crate::memory;
use crate::router::{self, Confidence, Route};
use crate::util::{append_jsonl, iso_now};

const MAX_MEMORY_RECORDS: usize = 3;
const MAX_GITNEXUS_FACTS: usize = 5;
const MAX_OUTPUT_WORDS: usize = 1200;

/// A structured context payload suitable for passing to the council binary.
#[derive(Debug, serde::Serialize)]
pub struct ContextPayload {
    pub schema_version: u32,
    pub task: String,
    pub route: String,
    pub confidence: String,
    pub memory_results: Vec<RetrievalItem>,
    pub graph_results: Vec<RetrievalItem>,
    pub retrieval_meta: RetrievalMeta,
}

#[derive(Debug, serde::Serialize)]
pub struct RetrievalItem {
    pub source: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct RetrievalMeta {
    pub memory_source: String,
    pub memory_latency_ms: u64,
    pub graph_latency_ms: u64,
    pub fallback_reason: Option<String>,
}

pub fn handle_query(task: &str, json_out: bool, no_audit: bool) -> Result<()> {
    let route_result = router::classify(task);

    // Low-confidence downgrades to neither (refusal bias)
    let effective_route = if route_result.confidence == Confidence::Low {
        Route::Neither
    } else {
        route_result.route
    };

    let mut memory_items: Vec<RetrievalItem> = Vec::new();
    let mut graph_items: Vec<RetrievalItem> = Vec::new();
    let mut open_uncertainty: Vec<String> = Vec::new();
    let mut memory_source = "none".to_string();
    let mut memory_latency_ms: u64 = 0;
    let mut graph_latency_ms: u64 = 0;
    let mut fallback_reason: Option<String> = None;

    // Retrieve memory if routed
    if matches!(effective_route, Route::MemoryOnly | Route::Both) {
        let t0 = Instant::now();
        match memory::retrieve_relevant(task, MAX_MEMORY_RECORDS) {
            Ok(records) if !records.is_empty() => {
                memory_source = "keyword".to_string();
                for r in &records {
                    memory_items.push(RetrievalItem {
                        source: r.source.clone(),
                        text: r.text.clone(),
                        timestamp: if r.timestamp.is_empty() {
                            None
                        } else {
                            Some(r.timestamp.clone())
                        },
                    });
                }
            }
            Ok(_) => {
                open_uncertainty.push("Memory retrieval returned no matching records.".into());
            }
            Err(e) => {
                open_uncertainty.push(format!("Memory retrieval failed: {e}"));
                fallback_reason = Some(format!("memory error: {e}"));
            }
        }
        memory_latency_ms = t0.elapsed().as_millis() as u64;
    }

    // Retrieve graph context if routed
    if matches!(effective_route, Route::GraphOnly | Route::Both) {
        let t0 = Instant::now();
        match graph::query(task, MAX_GITNEXUS_FACTS) {
            Ok(facts) if !facts.is_empty() => {
                for f in &facts {
                    graph_items.push(RetrievalItem {
                        source: "gitnexus".to_string(),
                        text: f.clone(),
                        timestamp: None,
                    });
                }
            }
            Ok(_) => {
                open_uncertainty.push(
                    "GitNexus query returned no results. Run `layers refresh` to update the index."
                        .into(),
                );
            }
            Err(e) => {
                open_uncertainty.push(format!("GitNexus retrieval failed: {e}"));
            }
        }
        graph_latency_ms = t0.elapsed().as_millis() as u64;
    }

    // Route-weighted interleave: prioritize the dominant signal's results
    let evidence_sections = interleave_results(effective_route, &memory_items, &graph_items);

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

    let retrieval_meta = RetrievalMeta {
        memory_source: memory_source.clone(),
        memory_latency_ms,
        graph_latency_ms,
        fallback_reason: fallback_reason.clone(),
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
            "retrieval": {
                "memory_source": memory_source,
                "memory_latency_ms": memory_latency_ms,
                "graph_latency_ms": graph_latency_ms,
                "memory_results": memory_items.len(),
                "graph_results": graph_items.len(),
                "fallback_reason": fallback_reason,
            },
        });
        let audit_path = memoryport_dir().join("layers-audit.jsonl");
        append_jsonl(&audit_path, &audit)?;
    }

    if json_out {
        let output = json!({
            "schema_version": CONTEXT_PAYLOAD_SCHEMA_VERSION,
            "route": effective_route.label(),
            "confidence": route_result.confidence.to_string(),
            "scores": route_result.scores,
            "why_retrieved": route_result.why,
            "why_not_retrieved": route_result.why_not,
            "evidence": final_evidence,
            "open_uncertainty": open_uncertainty,
            "retrieval_meta": retrieval_meta,
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

/// Route-weighted interleave:
/// - memory_only → memory first, graph as supplement
/// - graph_only → graph first, memory as supplement
/// - both → alternate memory/graph by position
fn interleave_results(
    route: Route,
    memory_items: &[RetrievalItem],
    graph_items: &[RetrievalItem],
) -> Vec<String> {
    let format_memory = |items: &[RetrievalItem]| -> Option<String> {
        if items.is_empty() {
            return None;
        }
        let lines: Vec<String> = items
            .iter()
            .map(|r| match &r.timestamp {
                Some(ts) => format!("- [{}][{}] {}", r.source, ts, r.text),
                None => format!("- [{}] {}", r.source, r.text),
            })
            .collect();
        Some(format!("### Memory\n{}", lines.join("\n")))
    };

    let format_graph = |items: &[RetrievalItem]| -> Option<String> {
        if items.is_empty() {
            return None;
        }
        let lines: Vec<String> = items.iter().map(|r| r.text.clone()).collect();
        Some(format!("### GitNexus\n{}", lines.join("\n")))
    };

    let mut sections = Vec::new();
    match route {
        Route::MemoryOnly | Route::Both if matches!(route, Route::MemoryOnly) => {
            if let Some(s) = format_memory(memory_items) {
                sections.push(s);
            }
            if let Some(s) = format_graph(graph_items) {
                sections.push(s);
            }
        }
        Route::GraphOnly => {
            if let Some(s) = format_graph(graph_items) {
                sections.push(s);
            }
            if let Some(s) = format_memory(memory_items) {
                sections.push(s);
            }
        }
        _ => {
            // Both: alternate — memory first, then graph
            if let Some(s) = format_memory(memory_items) {
                sections.push(s);
            }
            if let Some(s) = format_graph(graph_items) {
                sections.push(s);
            }
        }
    }
    sections
}

/// Build a ContextPayload for passing to the council binary.
pub fn build_context_payload(
    task: &str,
    route: Route,
    confidence: &str,
    memory_items: Vec<RetrievalItem>,
    graph_items: Vec<RetrievalItem>,
    retrieval_meta: RetrievalMeta,
) -> ContextPayload {
    ContextPayload {
        schema_version: CONTEXT_PAYLOAD_SCHEMA_VERSION,
        task: task.to_string(),
        route: route.label().to_string(),
        confidence: confidence.to_string(),
        memory_results: memory_items,
        graph_results: graph_items,
        retrieval_meta,
    }
}
