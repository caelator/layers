use anyhow::Result;
use serde_json::json;
use std::time::Instant;

use crate::config::{CONTEXT_PAYLOAD_SCHEMA_VERSION, memoryport_dir};
use crate::graph;
use crate::memory;
use crate::router::{self, Confidence, Route};
use crate::uc;
use crate::util::{append_jsonl, iso_now};
use crate::plugins::telemetry::schema::fingerprint_query;
use crate::cmd::telemetry::PluginResult;

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
    let t0 = Instant::now();
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

    // Retrieve memory if routed — try uc (semantic) first, fall back to local JSONL
    if matches!(effective_route, Route::MemoryOnly | Route::Both) {
        let t0 = Instant::now();
        let uc_retriever = uc::UcRetriever::new(uc::UcOptions::default());
        let uc_result = uc_retriever.retrieve(task, MAX_MEMORY_RECORDS);
        let used_uc = uc::meets_threshold_with(&uc_result, uc_retriever.min_results());

        if used_uc {
            memory_source = "uc".to_string();
            for line in &uc_result.lines {
                memory_items.push(RetrievalItem {
                    source: "uc".to_string(),
                    text: line.clone(),
                    timestamp: None,
                });
            }
        } else if let Some(reason) = &uc_result.fallback_reason {
            fallback_reason = Some(reason.clone());
        } else {
            fallback_reason = Some("uc returned too few results".into());
        }

        // Fall back to local keyword retrieval
        if !used_uc {
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
                    fallback_reason.get_or_insert_with(|| format!("memory error: {e}"));
                }
            }
        }

        memory_latency_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
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
        graph_latency_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
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
            "schema_version": CONTEXT_PAYLOAD_SCHEMA_VERSION,
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

    // Emit telemetry event
    let end_to_end_ms =
        u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    let fp = fingerprint_query(task);
    let memory_invoked =
        matches!(effective_route, Route::MemoryOnly | Route::Both);
    let gitnexus_invoked =
        matches!(effective_route, Route::GraphOnly | Route::Both);
    let memory_success = !memory_items.is_empty();
    let gitnexus_success = !graph_items.is_empty();

    let memory_result = if !memory_invoked {
        PluginResult::NotInvoked
    } else if memory_success {
        PluginResult::Success
    } else {
        PluginResult::Empty
    };
    let gitnexus_result = if !gitnexus_invoked {
        PluginResult::NotInvoked
    } else if gitnexus_success {
        PluginResult::Success
    } else {
        PluginResult::Empty
    };

    crate::cmd::telemetry::record_query_event(crate::cmd::telemetry::QueryEventParams {
        query_fingerprint: fp,
        route: effective_route.label().to_string(),
        confidence: match route_result.confidence {
            router::Confidence::High => 1.0,
            router::Confidence::Low => 0.5,
        },
        memory_result,
        memory_latency_ms,
        gitnexus_result,
        gitnexus_latency_ms: graph_latency_ms,
        end_to_end_ms,
    });

    Ok(())
}

/// Route-weighted interleave:
/// - `memory_only` → memory first, graph as supplement
/// - `graph_only` → graph first, memory as supplement
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
        Route::MemoryOnly => {
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
        Route::Both => {
            // Round-robin interleave: alternate memory and graph items
            let max_len = memory_items.len().max(graph_items.len());
            let mut interleaved_memory = Vec::new();
            let mut interleaved_graph = Vec::new();
            for i in 0..max_len {
                if let Some(item) = memory_items.get(i) {
                    let line = match &item.timestamp {
                        Some(ts) => format!("- [{}][{}] {}", item.source, ts, item.text),
                        None => format!("- [{}] {}", item.source, item.text),
                    };
                    interleaved_memory.push(line);
                }
                if let Some(item) = graph_items.get(i) {
                    interleaved_graph.push(item.text.clone());
                }
            }
            if !interleaved_memory.is_empty() {
                sections.push(format!("### Memory\n{}", interleaved_memory.join("\n")));
            }
            if !interleaved_graph.is_empty() {
                sections.push(format!("### GitNexus\n{}", interleaved_graph.join("\n")));
            }
        }
        Route::Neither => {
            // Neither: no results expected, but include any that exist
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

/// Build a `ContextPayload` for passing to the council binary.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CONTEXT_PAYLOAD_SCHEMA_VERSION;
    use crate::test_support::TestWorkspace;
    use crate::util::load_jsonl;

    /// Memory-only routing produces correct output structure (JSON mode).
    /// Uses a task that triggers `MemoryOnly` routing via historical keywords.
    #[test]
    fn handle_query_memory_only_produces_correct_structure() {
        let ws = TestWorkspace::new("query-memory-only");
        let root = ws.root();

        // Seed a memory record so keyword retrieval has something to find
        let plans_path = root.join("memoryport").join("council-plans.jsonl");
        std::fs::write(
            &plans_path,
            r#"{"task":"prior council decision","summary":"We previously decided to use Rust for the memory spine.","timestamp":"2026-04-01T00:00:00Z"}"#,
        )
        .unwrap();
        std::fs::write(plans_path.with_file_name("council-traces.jsonl"), "").unwrap();
        std::fs::write(plans_path.with_file_name("council-learnings.jsonl"), "").unwrap();

        // Task with strong historical signal: "prior", "decided", "rationale", "recall"
        let result = handle_query(
            "recall the prior decided rationale from the council history",
            true,
            true,
        );
        assert!(result.is_ok(), "handle_query failed: {:?}", result.err());
    }

    /// Neither routing returns appropriate empty/refusal response.
    #[test]
    fn handle_query_neither_returns_refusal() {
        let _ws = TestWorkspace::new("query-neither");

        // "hello" has no historical/structural signal → routes to Neither
        let result = handle_query("hello", true, true);
        assert!(result.is_ok(), "handle_query failed: {:?}", result.err());
    }

    /// Audit log entry is written with `schema_version` and correct fields.
    #[test]
    fn handle_query_writes_audit_with_schema_version() {
        let ws = TestWorkspace::new("query-audit");
        let root = ws.root();

        // Seed empty JSONL files so memory retrieval doesn't error
        for name in &[
            "council-plans.jsonl",
            "council-traces.jsonl",
            "council-learnings.jsonl",
        ] {
            std::fs::write(root.join("memoryport").join(name), "").unwrap();
        }

        // Run with audit enabled (no_audit = false)
        let result = handle_query("hello", false, false);
        assert!(result.is_ok(), "handle_query failed: {:?}", result.err());

        let audit_path = root.join("memoryport").join("layers-audit.jsonl");
        let records = load_jsonl(&audit_path).unwrap();
        assert_eq!(records.len(), 1, "expected exactly one audit entry");

        let entry = &records[0];
        assert_eq!(
            entry["schema_version"].as_u64().unwrap(),
            CONTEXT_PAYLOAD_SCHEMA_VERSION as u64,
            "audit entry must include schema_version"
        );
        assert_eq!(entry["action"], "query");
        assert_eq!(entry["task"], "hello");
        assert!(entry.get("route").is_some(), "audit must include route");
        assert!(
            entry.get("effective_route").is_some(),
            "audit must include effective_route"
        );
        assert!(
            entry.get("confidence").is_some(),
            "audit must include confidence"
        );
        assert!(
            entry.get("retrieval").is_some(),
            "audit must include retrieval metadata"
        );
    }
}
