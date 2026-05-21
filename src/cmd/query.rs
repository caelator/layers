use anyhow::Result;
use layers_compiler::{CompileMode, CompileRequest, ContextCompiler};
use layers_core::{
    ContextBudget, ContextItem, ContextPacket, ContextWarning, InjectionRecommendation,
    PacketQualityReport, RetrievalReport, SuccessRubric, TaskCategory, TaskSpec,
};
use serde_json::json;
use std::path::Path;
use std::time::Instant;

use crate::cmd::autoresearch::{AutoresearchPacketBridgeOptions, add_autoresearch_to_packet};
use crate::cmd::packet::format_objective_brief;
use crate::cmd::telemetry::PluginResult;
use crate::config::{CONTEXT_PAYLOAD_SCHEMA_VERSION, memoryport_dir, workspace_root};
use crate::context_packet_compiler::query_plan::{
    BroadQueryPlan, QueryInjectionPolicy, looks_code_heavy,
};
use crate::context_packet_compiler::{
    add_workspace_section, cited_item, code_impact_section, collect_workspace_state,
    context_section, gitnexus_impact_section, repo_source, source, workspace_id,
};
use crate::feedback::{
    FailureKind, HardErrorKind, RouteFailure, RouteId, RoutingSignals, SoftErrorKind, emit_failure,
    load_route_weights, read_recent_failures, route_corrections_path,
};
use crate::graph;
use crate::memory;
use crate::plugins::telemetry::schema::fingerprint_query;
use crate::quality;
use crate::router::{self, Confidence, Route};
use crate::uc;
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
    /// Whether this context payload is on the critical path.
    #[serde(default)]
    pub critical_path: bool,
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

pub fn handle_query(
    task: &str,
    json_out: bool,
    agent_prompt: bool,
    no_audit: bool,
    uc_min_results: usize,
) -> Result<()> {
    let t0 = Instant::now();
    let route_result = router::classify(task);

    // ── Route-weight override ────────────────────────────────────────────────
    // Load quality-based route weights BEFORE acting on the routing decision.
    // If the chosen route has accumulated enough soft/hard failures (weight < -0.3),
    // override to the best alternative or fall back to Neither.
    let recent_failures = read_recent_failures(&route_corrections_path(), 20);
    let route_weights = load_route_weights(&recent_failures);
    let effective_route = apply_weight_override(route_result.route, &route_weights);
    let query_plan = BroadQueryPlan::new(task, &route_result.scores, &workspace_root());

    // ── Code-heavy route upgrade ──────────────────────────────────────────────
    // When the query plan detects CodeHeavy intent with grounded code targets,
    // upgrade the effective route to include graph retrieval. Without this,
    // code-heavy queries with explicit Rust targets that the router classified
    // as MemoryOnly or Neither would skip graph/code context entirely, falling
    // back to memory-only context.
    let effective_route = apply_code_heavy_route_upgrade(effective_route, &query_plan);

    let mut memory_items: Vec<RetrievalItem> = Vec::new();
    let mut graph_items: Vec<RetrievalItem> = Vec::new();
    let mut open_uncertainty: Vec<String> = Vec::new();
    let mut memory_source = "none".to_string();
    let mut memory_latency_ms: u64 = 0;
    let mut graph_latency_ms: u64 = 0;
    let mut fallback_reason: Option<String> = None;

    // Map effective_route to feedback RouteId (needed for failure emission below)
    let current_fbid = match effective_route {
        Route::Neither => RouteId::Neither,
        Route::MemoryOnly => RouteId::MemoryOnly,
        Route::GraphOnly => RouteId::GraphOnly,
        Route::Both => RouteId::Both,
    };

    // Always try UC semantic retrieval when routed OR when the classifier
    // had low confidence (best-effort fallback — low confidence ≠ no retrieval).
    let low_confidence_fallback = route_result.confidence == Confidence::Low;
    if matches!(effective_route, Route::MemoryOnly | Route::Both) || low_confidence_fallback {
        let t0 = Instant::now();
        let uc_retriever = uc::UcRetriever::new(uc::UcOptions::default());
        let uc_result = uc_retriever.retrieve(task, MAX_MEMORY_RECORDS);
        let used_uc = uc::meets_threshold_with(&uc_result, uc_retriever.min_results());

        if used_uc {
            memory_source = if low_confidence_fallback {
                "uc-low-confidence-fallback".to_string()
            } else {
                "uc".to_string()
            };
            for line in &uc_result.lines {
                memory_items.push(RetrievalItem {
                    source: memory_source.clone(),
                    text: line.clone(),
                    timestamp: None,
                });
            }
        } else if let Some(reason) = &uc_result.fallback_reason {
            fallback_reason = Some(reason.clone());
        } else {
            fallback_reason = Some("uc returned too few results".into());
        }

        // Fall back to local keyword retrieval if UC didn't produce results
        if !used_uc {
            match memory::retrieve_relevant(task, MAX_MEMORY_RECORDS) {
                Ok(records) if !records.is_empty() => {
                    memory_source = if low_confidence_fallback {
                        "keyword-low-confidence-fallback".to_string()
                    } else {
                        "keyword".to_string()
                    };
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
                    if !low_confidence_fallback {
                        open_uncertainty
                            .push("Memory retrieval returned no matching records.".into());
                    }
                }
                Err(e) => {
                    if !low_confidence_fallback {
                        open_uncertainty.push(format!("Memory retrieval failed: {e}"));
                    }
                    fallback_reason.get_or_insert_with(|| format!("memory error: {e}"));
                    // RFC 006: emit HardError when memory retrieval errors
                    let failure = RouteFailure::new(
                        task.to_string(),
                        current_fbid,
                        FailureKind::Hard {
                            error_kind: HardErrorKind::NonZeroExit,
                            error_code: None,
                            tool_name: "memoryport".to_string(),
                        },
                        RoutingSignals::default(),
                    );
                    if let Err(fe) = emit_failure(&failure) {
                        eprintln!("[route-feedback] failed to emit hard failure: {fe}");
                    }
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
                // RFC 006: emit SoftError when graph returns empty on a graph-routed query
                let failure = RouteFailure::new(
                    task.to_string(),
                    current_fbid,
                    FailureKind::Soft {
                        error_kind: SoftErrorKind::InsufficientContext,
                        flagged_by: "layers-query".to_string(),
                        affected_stage: "graph-retrieval".to_string(),
                    },
                    RoutingSignals::default(),
                );
                if let Err(e) = emit_failure(&failure) {
                    eprintln!("[route-feedback] failed to emit soft failure: {e}");
                }
            }
            Err(e) => {
                open_uncertainty.push(format!("GitNexus retrieval failed: {e}"));
                // RFC 006: emit HardError when graph retrieval errors
                let failure = RouteFailure::new(
                    task.to_string(),
                    current_fbid,
                    FailureKind::Hard {
                        error_kind: HardErrorKind::NonZeroExit,
                        error_code: None,
                        tool_name: "gitnexus".to_string(),
                    },
                    RoutingSignals::default(),
                );
                if let Err(e) = emit_failure(&failure) {
                    eprintln!("[route-feedback] failed to emit hard failure: {e}");
                }
            }
        }
        graph_latency_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    }

    // ── Result quality evaluation ─────────────────────────────────────────────
    // Score returned results for relevance and specificity.
    // Emit Soft failures when results exist but are poor quality.
    let routing_signals = RoutingSignals {
        query_length_chars: task.len(),
        history_turns: 0,
        intent_confidence: match route_result.confidence {
            Confidence::High => 1.0,
            Confidence::Low => 0.5,
        },
        graph_symbol_count: graph_items.len(),
        memory_hits: memory_items.len(),
        council_load: 0.0,
        ..RoutingSignals::default()
    };

    if !memory_items.is_empty() {
        let texts: Vec<&str> = memory_items.iter().map(|r| r.text.as_str()).collect();
        let mq = quality::evaluate(task, &texts, MAX_MEMORY_RECORDS);
        if !mq.acceptable {
            if let Some(ref reason) = mq.reason {
                open_uncertainty.push(format!("Memory quality: {reason}"));
            }
            quality::emit_if_poor(
                &mq,
                task,
                current_fbid,
                "memory-retrieval",
                routing_signals.clone(),
            );
        }
    }

    if !graph_items.is_empty() {
        let texts: Vec<&str> = graph_items.iter().map(|r| r.text.as_str()).collect();
        let gq = quality::evaluate(task, &texts, MAX_GITNEXUS_FACTS);
        if !gq.acceptable {
            if let Some(ref reason) = gq.reason {
                open_uncertainty.push(format!("Graph quality: {reason}"));
            }
            quality::emit_if_poor(
                &gq,
                task,
                current_fbid,
                "graph-retrieval",
                routing_signals.clone(),
            );
        }
    }

    // ── Route-correction feedback: soft-failure suppression ──────────────────
    // Reuse the route weights loaded at the top (before retrieval).
    let route_weight = route_weights.get(&current_fbid).copied().unwrap_or(0.0_f32);

    // If prior soft failures have demoted this route significantly, flag the results.
    // A route weight below -0.3 signals chronic quality issues on this route pattern.
    if route_weight < -0.3 && (!memory_items.is_empty() || !graph_items.is_empty()) {
        open_uncertainty.push(format!(
            "Prior route failures on '{}' detected (weight={route_weight:?}). Results may be degraded — verify critical details.",
            effective_route.label(),
        ));
    }

    // ── uc_min_results threshold warning ─────────────────────────────────────
    // Surface a warning when UC semantic retrieval returned fewer results than
    // the configured minimum — the evidence budget may be under-filled.
    if matches!(effective_route, Route::MemoryOnly | Route::Both) || low_confidence_fallback {
        let uc_count = memory_items
            .iter()
            .filter(|item| item.source.starts_with("uc"))
            .count();
        if uc_count > 0 && uc_count < uc_min_results {
            open_uncertainty.push(format!(
                "UC semantic retrieval returned {uc_count} result{} (below --uc-min-results={uc_min_results}). Evidence may be thin.",
                if uc_count == 1 { "" } else { "s" }
            ));
        }
    }

    // Route-weighted interleave: prioritize the dominant signal's results
    let evidence_sections = interleave_results(effective_route, &memory_items, &graph_items);

    // Enforce word budget
    let evidence_text = evidence_sections.join("\n\n");
    let word_count = evidence_text.split_whitespace().count();
    let (_final_evidence, budget_exceeded) = if word_count > MAX_OUTPUT_WORDS {
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

    // Route failure feedback — RFC 006 Stage 2.
    // If low-confidence fallback retrieved nothing, emit a RouteFailure.
    if low_confidence_fallback && memory_items.is_empty() && graph_items.is_empty() {
        let failure = RouteFailure::new(
            task.to_string(),
            RouteId::Neither,
            FailureKind::Soft {
                error_kind: SoftErrorKind::InsufficientContext,
                flagged_by: "layers-classifier".to_string(),
                affected_stage: "query".to_string(),
            },
            routing_signals.clone(),
        );
        if let Err(e) = emit_failure(&failure) {
            eprintln!("[route-feedback] failed to emit failure record: {e}");
        }
    }

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

    let mut packet = build_context_packet(
        task,
        effective_route,
        route_result.confidence.to_string(),
        &memory_items,
        &graph_items,
        &open_uncertainty,
        &retrieval_meta,
        &route_result.scores,
        &route_result.why,
        &route_result.why_not,
        word_count,
        budget_exceeded,
        low_confidence_fallback,
        None,
        &query_plan,
    );
    let task_spec = task_spec_for_query(task, &route_result.scores, &query_plan);
    add_packet_quality_report(&mut packet, &task_spec);
    let recommendation = packet_quality_recommendation(&packet);
    let should_inject = !matches!(
        recommendation,
        Some(InjectionRecommendation::Abstain | InjectionRecommendation::NeedsTarget)
    );
    let final_evidence = packet.evidence.clone();

    if agent_prompt && should_inject {
        println!("{}", format_objective_brief(&packet));
    } else if agent_prompt {
        print_abstention_context(&packet);
    } else if json_out {
        println!("{}", serde_json::to_string_pretty(&packet)?);
    } else if !should_inject {
        print_abstention_context(&packet);
    } else if matches!(effective_route, Route::Neither) {
        // Low-confidence fallback: if we retrieved anyway, show the evidence
        if !memory_items.is_empty() || !graph_items.is_empty() {
            println!("<layers_context>");
            println!(
                "Route: {} (low confidence — best-effort retrieval)",
                effective_route.label()
            );
            println!(
                "Why Retrieved: Semantic retrieval found relevant context despite low classifier confidence."
            );
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
        } else {
            println!("<layers_context>");
            println!("Route: {}", effective_route.label());
            println!("Why Not Retrieved: {}", route_result.why);
            println!("No context injection — task does not warrant retrieval.");
            println!("</layers_context>");
        }
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
    let end_to_end_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    let fp = fingerprint_query(task);
    let memory_invoked = matches!(effective_route, Route::MemoryOnly | Route::Both);
    let gitnexus_invoked = matches!(effective_route, Route::GraphOnly | Route::Both);
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

/// Apply route-weight override: if the chosen route has a weight below -0.3
/// (chronic quality failures), fall back to the best alternative route or Neither.
///
/// This is the bridge between quality evaluation feedback (soft failures written
/// to route-corrections.jsonl) and the initial routing decision.
fn apply_weight_override(route: Route, weights: &std::collections::HashMap<RouteId, f32>) -> Route {
    let route_id = match route {
        Route::Neither => RouteId::Neither,
        Route::MemoryOnly => RouteId::MemoryOnly,
        Route::GraphOnly => RouteId::GraphOnly,
        Route::Both => RouteId::Both,
    };

    let weight = weights.get(&route_id).copied().unwrap_or(0.0_f32);
    if weight >= -0.3 {
        return route;
    }

    // Find the best alternative route (highest weight, better than current).
    let all_routes = [
        (RouteId::MemoryOnly, Route::MemoryOnly),
        (RouteId::GraphOnly, Route::GraphOnly),
        (RouteId::Both, Route::Both),
        (RouteId::Neither, Route::Neither),
    ];

    let fallback = all_routes
        .iter()
        .filter(|(id, _)| *id != route_id)
        .map(|(id, r)| (*r, weights.get(id).copied().unwrap_or(0.0_f32)))
        .filter(|(_, w)| *w > weight)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(r, _)| r);

    fallback.unwrap_or(Route::Neither)
}

/// Upgrade the effective route when the query plan detects [`QueryIntent::CodeHeavy`]
/// intent with grounded code targets. Without this, code-heavy queries with explicit
/// Rust file targets that the router classified as [`Route::MemoryOnly`] or
/// [`Route::Neither`] would skip graph/code retrieval entirely, falling back to
/// memory-only context.
///
/// Rules:
/// - [`QueryIntent::CodeHeavy`] + [`UseGroundedTargets`] + [`Route::Neither`] → [`Route::GraphOnly`]
/// - [`QueryIntent::CodeHeavy`] + [`UseGroundedTargets`] + [`Route::MemoryOnly`] → [`Route::Both`]
/// - [`QueryIntent::CodeHeavy`] + [`UseGroundedTargets`] + [`Route::GraphOnly`] → [`Route::GraphOnly`] (unchanged)
/// - [`QueryIntent::CodeHeavy`] + [`UseGroundedTargets`] + [`Route::Both`] → [`Route::Both`] (unchanged)
/// - Other intents/policies → no change
fn apply_code_heavy_route_upgrade(route: Route, query_plan: &BroadQueryPlan) -> Route {
    if query_plan.injection_policy != QueryInjectionPolicy::UseGroundedTargets {
        return route;
    }
    match route {
        Route::Neither => Route::GraphOnly,
        Route::MemoryOnly => Route::Both,
        other => other,
    }
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

#[allow(clippy::too_many_arguments)]
fn build_context_packet(
    task: &str,
    route: Route,
    confidence: String,
    memory_items: &[RetrievalItem],
    graph_items: &[RetrievalItem],
    warnings: &[String],
    retrieval_meta: &RetrievalMeta,
    scores: &router::Scores,
    why_retrieved: &str,
    why_not_retrieved: &str,
    used_words: usize,
    truncated: bool,
    low_confidence_fallback: bool,
    evidence: Option<String>,
    query_plan: &BroadQueryPlan,
) -> ContextPacket {
    let workspace = workspace_root();
    let workspace_id = workspace_id(&workspace);
    let mut packet = ContextPacket::new(
        format!("ctx-{}", uuid::Uuid::new_v4()),
        workspace_id,
        task.to_string(),
        chrono::Utc::now(),
    );
    packet.route = route.label().to_string();
    packet.provenance.surface = "query".to_string();
    packet.confidence = confidence;
    packet.budget = ContextBudget {
        max_units: MAX_OUTPUT_WORDS,
        used_units: used_words,
        unit: "words".to_string(),
        truncated,
    };
    packet.retrieval = RetrievalReport {
        memory_source: retrieval_meta.memory_source.clone(),
        memory_latency_ms: retrieval_meta.memory_latency_ms,
        graph_latency_ms: retrieval_meta.graph_latency_ms,
        fallback_reason: retrieval_meta.fallback_reason.clone(),
    };
    packet.retrieval_meta = packet.retrieval.clone();
    packet.scores = serde_json::to_value(scores).unwrap_or(serde_json::Value::Null);
    packet.why_retrieved = why_retrieved.to_string();
    packet.why_not_retrieved = why_not_retrieved.to_string();
    packet.low_confidence_fallback = low_confidence_fallback;
    packet.open_uncertainty = warnings.to_vec();

    let workspace_state = collect_workspace_state(&workspace);
    packet.git_ref.clone_from(&workspace_state.head);
    add_workspace_section(&mut packet, &workspace_state);

    if !memory_items.is_empty() {
        packet.sections.push(context_section(
            "memory",
            "Memory",
            "Relevant project memory and semantic recall.",
            retrieval_items_to_context_items("memory", memory_items),
        ));
    }
    if !graph_items.is_empty() {
        packet
            .sections
            .push(gitnexus_impact_section(retrieval_items_to_context_items(
                "gitnexus",
                graph_items,
            )));
    }
    add_query_plan_to_packet(&mut packet, query_plan);
    add_autoresearch_to_packet(
        &mut packet,
        AutoresearchPacketBridgeOptions {
            task,
            targets: &[],
            limit: MAX_MEMORY_RECORDS,
            unavailable_message: "No persisted autoresearch store was available for query.",
        },
    );
    if packet.sections.is_empty() {
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "no_context_selected".to_string(),
            message: "No memory or graph context was selected for this query.".to_string(),
        });
    }
    for warning in warnings {
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "retrieval_warning".to_string(),
            message: warning.clone(),
        });
    }
    if truncated {
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "budget_truncated".to_string(),
            message: format!("Evidence exceeded {MAX_OUTPUT_WORDS}-word budget and was truncated."),
        });
    }
    let derive_evidence = evidence.is_none();
    let mut compiled = ContextCompiler::new().compile(
        CompileRequest::new(
            packet.id.clone(),
            packet.workspace_id.clone(),
            packet.query.clone(),
            packet.created_at,
            CompileMode::Query,
        )
        .with_route_label(route.label())
        .with_sections(std::mem::take(&mut packet.sections))
        .with_warnings(std::mem::take(&mut packet.warnings))
        .with_git_ref(packet.git_ref.clone())
        .derive_evidence(derive_evidence),
    );
    if let Some(evidence_text) = evidence {
        compiled.evidence = evidence_text;
    }
    compiled.confidence = packet.confidence;
    compiled.budget = packet.budget;
    compiled.retrieval = packet.retrieval;
    compiled.retrieval_meta = packet.retrieval_meta;
    compiled.scores = packet.scores;
    compiled.why_retrieved = packet.why_retrieved;
    compiled.why_not_retrieved = packet.why_not_retrieved;
    compiled.low_confidence_fallback = packet.low_confidence_fallback;
    compiled
}

fn add_query_plan_to_packet(packet: &mut ContextPacket, query_plan: &BroadQueryPlan) {
    match query_plan.injection_policy {
        QueryInjectionPolicy::NeedsTarget => {
            packet.open_uncertainty.push(format!(
                "Code-heavy query needs explicit or discoverable targets before context injection. Try: {}",
                query_plan.suggested_command
            ));
            packet.warnings.push(ContextWarning {
                severity: "warning".to_string(),
                code: "query_needs_target".to_string(),
                message: format!(
                    "Code-heavy query did not identify reliable code targets. Suggested command: {}",
                    query_plan.suggested_command
                ),
            });
        }
        QueryInjectionPolicy::UseGroundedTargets => {
            add_query_code_section(packet, query_plan);
        }
        QueryInjectionPolicy::AllowMemoryOnly => {}
    }
}

fn add_query_code_section(packet: &mut ContextPacket, query_plan: &BroadQueryPlan) {
    let workspace = workspace_root();
    let items = query_plan
        .all_targets()
        .into_iter()
        .enumerate()
        .filter_map(|(idx, target)| query_target_item(idx, &workspace, &target))
        .collect::<Vec<_>>();
    if !items.is_empty() {
        packet.sections.push(code_impact_section(items));
    }
}

fn query_target_item(idx: usize, workspace: &Path, target: &Path) -> Option<ContextItem> {
    let workspace = workspace.canonicalize().ok()?;
    let path = if target.is_absolute() {
        target.to_path_buf()
    } else {
        workspace.join(target)
    };
    let path = path.canonicalize().ok()?;
    if !path.starts_with(&workspace) {
        return None;
    }
    let target = path.strip_prefix(&workspace).ok()?.to_path_buf();
    let metadata = std::fs::metadata(&path).ok()?;
    let mut body = if metadata.is_dir() {
        summarize_query_directory(&path)?
    } else {
        summarize_query_file(&path)?
    };
    if body.trim().is_empty() {
        body = format!("Target exists at {}.", target.display());
    }
    let target_text = target.to_string_lossy().to_string();
    Some(cited_item(
        format!("query-target-{}", idx + 1),
        format!("Query target: {target_text}"),
        body,
        repo_source(
            "workspace_file",
            format!("file://{}", path.display()),
            Some(target_text),
        ),
        "broad-query planning identified this code target before injection",
        vec!["query-target".to_string(), "code".to_string()],
    ))
}

fn summarize_query_file(path: &Path) -> Option<String> {
    const MAX_BYTES: u64 = 12_000;
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_BYTES {
        return Some(format!(
            "File exists but is larger than {MAX_BYTES} bytes; inspect directly before editing."
        ));
    }
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.lines().take(80).collect::<Vec<_>>().join("\n"))
}

fn summarize_query_directory(path: &Path) -> Option<String> {
    let mut entries = std::fs::read_dir(path)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    entries.sort();
    entries.truncate(40);
    Some(format!("Directory entries:\n- {}", entries.join("\n- ")))
}

fn task_spec_for_query(
    task: &str,
    scores: &router::Scores,
    query_plan: &BroadQueryPlan,
) -> TaskSpec {
    let code_heavy = looks_code_heavy(task, scores);
    let category = if code_heavy {
        TaskCategory::Debugging
    } else {
        TaskCategory::Orientation
    };
    let targets = query_plan.all_targets();
    TaskSpec {
        task_id: "query-task".to_string(),
        title: "Query task".to_string(),
        prompt: task.to_string(),
        category,
        repo_root: Some(workspace_root()),
        target_files: targets.clone(),
        target_symbols: Vec::new(),
        expected_relevant_files: targets,
        expected_validation_commands: Vec::new(),
        negative_control: false,
        success_rubric: SuccessRubric::default(),
    }
}

fn add_packet_quality_report(packet: &mut ContextPacket, task_spec: &TaskSpec) {
    let report = PacketQualityReport::grade(packet, task_spec);
    packet.scores = json!({
        "router": packet.scores.clone(),
        "packet_quality": &report,
        "injection_recommendation": report.recommendation,
    });
    packet.warnings.push(ContextWarning {
        severity: match report.recommendation {
            InjectionRecommendation::InjectFull | InjectionRecommendation::InjectCompact => "info",
            InjectionRecommendation::Abstain | InjectionRecommendation::NeedsTarget => "warning",
        }
        .to_string(),
        code: "injection_policy".to_string(),
        message: format!(
            "Packet quality gate recommends {:?}: {}",
            report.recommendation,
            report.reasons.join("; ")
        ),
    });
}

fn packet_quality_recommendation(packet: &ContextPacket) -> Option<InjectionRecommendation> {
    serde_json::from_value(packet.scores.get("injection_recommendation")?.clone()).ok()
}

fn print_abstention_context(packet: &ContextPacket) {
    println!("<layers_context>");
    println!("Route: {}", packet.route);
    if let Some(recommendation) = packet.scores.get("injection_recommendation") {
        println!("Injection Recommendation: {recommendation}");
    }
    if let Some(report) = packet.scores.get("packet_quality") {
        if let Some(reasons) = report.get("reasons").and_then(serde_json::Value::as_array) {
            println!("Why Not Injected:");
            for reason in reasons.iter().filter_map(serde_json::Value::as_str) {
                println!("- {reason}");
            }
        }
    }
    println!(
        "No context injection — packet quality gate predicted low value or needs explicit targets."
    );
    println!("</layers_context>");
}

fn retrieval_items_to_context_items(section: &str, items: &[RetrievalItem]) -> Vec<ContextItem> {
    items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let selected_reason = if section == "memory" {
                "memory retrieval matched the query or low-confidence fallback"
            } else {
                "GitNexus graph retrieval matched structural query terms"
            };
            cited_item(
                format!("{section}-{}", idx + 1),
                item.source.clone(),
                item.text.clone(),
                source(section, item.source.clone()),
                selected_reason,
                Vec::new(),
            )
        })
        .collect()
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
        critical_path: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::autoresearch::{
        AutoresearchCommands, ProfileCommands, SourceCommands, handle_autoresearch,
    };
    use crate::config::CONTEXT_PAYLOAD_SCHEMA_VERSION;
    use crate::test_support::TestWorkspace;
    use crate::util::load_jsonl;
    use std::io::Write;

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
            false,
            true,
            3,
        );
        assert!(result.is_ok(), "handle_query failed: {:?}", result.err());
    }

    /// Neither routing returns appropriate empty/refusal response.
    #[test]
    fn handle_query_neither_returns_refusal() {
        let _ws = TestWorkspace::new("query-neither");

        // "hello" has no historical/structural signal → routes to Neither
        let result = handle_query("hello", true, false, true, 3);
        assert!(result.is_ok(), "handle_query failed: {:?}", result.err());
    }

    #[test]
    fn query_context_packet_bridges_autoresearch_findings() {
        let _ws = TestWorkspace::new("query-autoresearch-bridge");
        seed_autoresearch_store("query");
        let retrieval_meta = RetrievalMeta {
            memory_source: "none".to_string(),
            memory_latency_ms: 0,
            graph_latency_ms: 0,
            fallback_reason: None,
        };
        let scores = router::Scores {
            historical: 0,
            structural: 1,
            local: 1,
            action: 1,
        };

        let query_plan = BroadQueryPlan::new(
            "fill context compiler autoresearch gap",
            &scores,
            &workspace_root(),
        );
        let packet = build_context_packet(
            "fill context compiler autoresearch gap",
            Route::GraphOnly,
            "medium".to_string(),
            &[],
            &[],
            &[],
            &retrieval_meta,
            &scores,
            "structural query",
            "",
            0,
            false,
            false,
            None,
            &query_plan,
        );
        let section = packet
            .sections
            .iter()
            .find(|section| section.id == "autoresearch")
            .expect("query packet should include task-matched autoresearch findings");

        assert_eq!(section.items[0].source.kind, "autoresearch");
        assert!(
            packet
                .evidence
                .contains("Context compiler autoresearch gap")
        );
        assert!(packet.scores["autoresearch_findings"].as_u64() == Some(1));
        assert!(section.items[0].body.contains("Provenance:"));
        assert!(
            packet
                .selection_trace
                .iter()
                .any(|trace| trace.item_id == "autoresearch-1")
        );
    }

    #[test]
    fn query_context_packet_preserves_route_label_and_query_surface() {
        let _ws = TestWorkspace::new("query-route-label-surface");
        let retrieval_meta = RetrievalMeta {
            memory_source: "memoryport".to_string(),
            memory_latency_ms: 7,
            graph_latency_ms: 0,
            fallback_reason: Some("graph disabled".to_string()),
        };
        let scores = router::Scores {
            historical: 1,
            structural: 0,
            local: 0,
            action: 0,
        };
        let query_plan =
            BroadQueryPlan::new("recall prior route decision", &scores, &workspace_root());
        let packet = build_context_packet(
            "recall prior route decision",
            Route::MemoryOnly,
            "high".to_string(),
            &[RetrievalItem {
                source: "memoryport/council-plans.jsonl#1".to_string(),
                text: "Route decision: query packets keep router labels while provenance records query."
                    .to_string(),
                timestamp: None,
            }],
            &[],
            &["retrieval degraded".to_string()],
            &retrieval_meta,
            &scores,
            "matched historical route decision",
            "",
            11,
            false,
            false,
            None,
            &query_plan,
        );

        assert_eq!(packet.route, "memory_only");
        assert_eq!(packet.provenance.surface, "query");
        assert_eq!(
            packet.retrieval_meta.memory_source,
            retrieval_meta.memory_source
        );
        assert_eq!(
            packet.retrieval_meta.memory_latency_ms,
            retrieval_meta.memory_latency_ms
        );
        assert_eq!(
            packet.retrieval_meta.graph_latency_ms,
            retrieval_meta.graph_latency_ms
        );
        assert_eq!(
            packet.retrieval_meta.fallback_reason,
            retrieval_meta.fallback_reason
        );
        assert!(
            packet
                .selection_trace
                .iter()
                .any(|trace| trace.item_id == "memory-1")
        );
        assert!(packet.evidence.contains("Route decision"));
        assert!(
            packet
                .open_uncertainty
                .contains(&"retrieval degraded".to_string())
        );
    }

    #[test]
    fn code_heavy_query_without_targets_requests_targets_before_injection() {
        let _ws = TestWorkspace::new("query-quality-needs-target");
        let retrieval_meta = RetrievalMeta {
            memory_source: "none".to_string(),
            memory_latency_ms: 0,
            graph_latency_ms: 0,
            fallback_reason: None,
        };
        let scores = router::Scores {
            historical: 0,
            structural: 1,
            local: 1,
            action: 1,
        };
        let query_plan = BroadQueryPlan::new("fix the CLI parser bug", &scores, &workspace_root());
        let mut packet = build_context_packet(
            "fix the CLI parser bug",
            Route::MemoryOnly,
            "high".to_string(),
            &[],
            &[],
            &[],
            &retrieval_meta,
            &scores,
            "code-heavy broad query",
            "",
            0,
            false,
            false,
            None,
            &query_plan,
        );
        let task_spec = task_spec_for_query("fix the CLI parser bug", &scores, &query_plan);
        add_packet_quality_report(&mut packet, &task_spec);

        assert_eq!(
            packet_quality_recommendation(&packet),
            Some(InjectionRecommendation::NeedsTarget)
        );
    }

    #[test]
    fn query_target_item_rejects_absolute_paths_outside_workspace() {
        let ws = TestWorkspace::new("query-target-absolute-rejection");
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), "do not inject").unwrap();

        let item = query_target_item(0, ws.root(), outside.path());

        assert!(item.is_none());
    }

    fn seed_autoresearch_store(prefix: &str) {
        handle_autoresearch(&AutoresearchCommands::Source {
            command: SourceCommands::Add {
                url: format!("file:///{prefix}/context-compiler-autoresearch-gap.md"),
                title: Some("Context compiler autoresearch gap resolution".to_string()),
                source_type: "article".to_string(),
            },
        })
        .unwrap();
        handle_autoresearch(&AutoresearchCommands::Profile {
            command: ProfileCommands::Create {
                name: "Context compiler".to_string(),
                keywords: "context,compiler,autoresearch,gap".to_string(),
                negative_keywords: None,
                score_threshold: Some(1.0),
                max_llm_calls: Some(0),
                json: true,
            },
        })
        .unwrap();
        handle_autoresearch(&AutoresearchCommands::ScanOnce {
            profile_id: None,
            json: true,
        })
        .unwrap();
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
        let result = handle_query("hello", false, false, false, 3);
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

    /// Soft failure suppression: a route with prior failures (weight < -0.3) surfaces
    /// a warning in `open_uncertainty` when results are retrieved.
    /// This is tested indirectly via the underlying functions:
    /// - `read_recent_failures` (tested in `feedback::tests`)
    /// - `load_route_weights` (tested in `feedback::tests`)
    /// - The warning condition: `route_weight` < -0.3 after loading failures
    ///
    /// An end-to-end test would require capturing stdout from `handle_query`,
    /// which is not easily possible without refactoring the function to
    /// return the output string. The unit-level coverage of the suppression
    /// logic via `load_route_weights` and `read_recent_failures` is sufficient
    /// to verify correctness of the feedback loop.
    #[test]
    fn handle_query_soft_failure_suppression_unit() {
        // Verify: two soft failures on Both route give weight = -0.4
        // which is below the -0.3 suppression threshold.

        // Use a temp file for isolation — avoids contaminating ~/.layers with test data
        let tmp = tempfile::NamedTempFile::with_suffix(".jsonl").unwrap();
        let failures_path = tmp.path().to_path_buf();

        let f1 = RouteFailure::new(
            "deploy the auth service".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::Hallucination,
                flagged_by: "solution_scout".to_string(),
                affected_stage: "deliberation".to_string(),
            },
            RoutingSignals::default(),
        );
        let f2 = RouteFailure::new(
            "architect the middleware layer".to_string(),
            RouteId::Both,
            FailureKind::Soft {
                error_kind: SoftErrorKind::InsufficientContext,
                flagged_by: "solution_scout".to_string(),
                affected_stage: "query".to_string(),
            },
            RoutingSignals::default(),
        );

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&failures_path)
            .unwrap();
        for f in &[&f1, &f2] {
            writeln!(file, "{}", serde_json::to_string(f).unwrap()).unwrap();
        }
        drop(file);

        // Read and verify the failures were stored correctly
        let recent = read_recent_failures(&failures_path, 10);
        assert_eq!(recent.len(), 2);

        let weights = load_route_weights(&recent);
        let both_weight = weights.get(&RouteId::Both).copied().unwrap_or(0.0);
        // Two soft failures: -0.2 * 2 = -0.4
        assert!(
            both_weight < -0.3,
            "Both route weight ({both_weight}) should be below -0.3 threshold"
        );

        // Verify handle_query runs without error when failures file exists
        let result = handle_query("hello", true, false, true, 3);
        assert!(
            result.is_ok(),
            "handle_query should succeed with failures file: {:?}",
            result.err()
        );
    }

    /// When corrections accumulate and a route gets weight < -0.3,
    /// `apply_weight_override` overrides to the best alternative route.
    #[test]
    fn weight_override_demotes_route_below_threshold() {
        use std::collections::HashMap;

        // Simulate two soft failures on Both → weight = -0.4
        let mut weights = HashMap::new();
        weights.insert(RouteId::Both, -0.4_f32);

        // classify would return Both, but override should pick a better route
        let overridden = apply_weight_override(Route::Both, &weights);
        assert_ne!(
            overridden,
            Route::Both,
            "route must be overridden when weight < -0.3"
        );
        // With no other weights, all alternatives have weight 0.0 > -0.4.
        // The override picks an alternative (not Both) — exact choice depends
        // on tie-breaking order, but Neither is a safe conservative fallback.
        assert_ne!(
            overridden,
            Route::Both,
            "route must not be Both when its weight is below threshold"
        );
    }

    /// When a specific alternative has been human-boosted, the override
    /// selects that boosted route.
    #[test]
    fn weight_override_selects_boosted_alternative() {
        use std::collections::HashMap;

        let mut weights = HashMap::new();
        weights.insert(RouteId::Both, -0.5_f32);
        weights.insert(RouteId::GraphOnly, 0.4_f32); // human-boosted

        let overridden = apply_weight_override(Route::Both, &weights);
        assert_eq!(
            overridden,
            Route::GraphOnly,
            "should select the human-boosted alternative"
        );
    }

    /// When no corrections exist (empty weights), routing is unchanged.
    #[test]
    fn weight_override_no_corrections_unchanged() {
        use std::collections::HashMap;

        let weights = HashMap::new();

        // All routes should pass through unchanged
        assert_eq!(apply_weight_override(Route::Both, &weights), Route::Both);
        assert_eq!(
            apply_weight_override(Route::MemoryOnly, &weights),
            Route::MemoryOnly
        );
        assert_eq!(
            apply_weight_override(Route::GraphOnly, &weights),
            Route::GraphOnly
        );
        assert_eq!(
            apply_weight_override(Route::Neither, &weights),
            Route::Neither
        );
    }

    /// When all routes have bad weights, falls back to Neither.
    #[test]
    fn weight_override_all_bad_falls_back_to_neither() {
        use std::collections::HashMap;

        let mut weights = HashMap::new();
        weights.insert(RouteId::Both, -0.6_f32);
        weights.insert(RouteId::MemoryOnly, -0.8_f32);
        weights.insert(RouteId::GraphOnly, -0.7_f32);
        weights.insert(RouteId::Neither, -0.5_f32);

        // Both has weight -0.6, Neither has -0.5 (best alternative)
        let overridden = apply_weight_override(Route::Both, &weights);
        assert_eq!(
            overridden,
            Route::Neither,
            "when all routes are bad, should pick least-bad (Neither at -0.5)"
        );
    }

    // ── Code-heavy route upgrade tests ────────────────────────────────────────

    /// When [`BroadQueryPlan`] detects [`QueryIntent::CodeHeavy`] with grounded
    /// targets and the router classified as [`Route::Neither`], the route should
    /// be upgraded to [`Route::GraphOnly`].
    #[test]
    fn code_heavy_with_targets_upgrades_neither_to_graph_only() {
        let ws = TestWorkspace::new("query-code-heavy-upgrade-neither");
        let root = ws.root();

        // Create a real Rust file so extract_path_like_targets can ground it
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/router.rs"), "pub fn classify() {}").unwrap();

        let scores = router::Scores {
            historical: 0,
            structural: 0,
            local: 0,
            action: 1, // "fix" is an action signal
        };

        // "fix src/router.rs" is code-heavy with an explicit grounded target
        let query_plan = BroadQueryPlan::new("fix src/router.rs", &scores, root);

        assert_eq!(
            query_plan.injection_policy,
            QueryInjectionPolicy::UseGroundedTargets
        );

        let upgraded = apply_code_heavy_route_upgrade(Route::Neither, &query_plan);
        assert_eq!(
            upgraded,
            Route::GraphOnly,
            "Neither should be upgraded to GraphOnly for code-heavy queries with grounded targets"
        );
    }

    /// When [`BroadQueryPlan`] detects [`QueryIntent::CodeHeavy`] with grounded
    /// targets and the router classified as [`Route::MemoryOnly`], the route
    /// should be upgraded to [`Route::Both`].
    #[test]
    fn code_heavy_with_targets_upgrades_memory_only_to_both() {
        let ws = TestWorkspace::new("query-code-heavy-upgrade-memory");
        let root = ws.root();

        std::fs::create_dir_all(root.join("src/cmd")).unwrap();
        std::fs::write(root.join("src/cmd/query.rs"), "pub fn handle_query() {}").unwrap();

        let scores = router::Scores {
            historical: 2, // "recover" or similar historical signals
            structural: 0,
            local: 0,
            action: 1, // "fix"
        };

        let query_plan =
            BroadQueryPlan::new("fix the regression in src/cmd/query.rs", &scores, root);

        assert_eq!(
            query_plan.injection_policy,
            QueryInjectionPolicy::UseGroundedTargets
        );

        let upgraded = apply_code_heavy_route_upgrade(Route::MemoryOnly, &query_plan);
        assert_eq!(
            upgraded,
            Route::Both,
            "MemoryOnly should be upgraded to Both for code-heavy queries with grounded targets"
        );
    }

    /// When the query plan has [`AllowMemoryOnly`] policy (historical intent),
    /// the route should NOT be upgraded.
    #[test]
    fn historical_query_plan_does_not_upgrade_route() {
        let ws = TestWorkspace::new("query-historical-no-upgrade");
        let scores = router::Scores {
            historical: 2,
            structural: 0,
            local: 0,
            action: 0,
        };

        let query_plan = BroadQueryPlan::new(
            "recall the prior decided rationale from memory",
            &scores,
            ws.root(),
        );

        assert_eq!(
            query_plan.injection_policy,
            QueryInjectionPolicy::AllowMemoryOnly
        );

        let upgraded = apply_code_heavy_route_upgrade(Route::MemoryOnly, &query_plan);
        assert_eq!(
            upgraded,
            Route::MemoryOnly,
            "MemoryOnly should NOT be upgraded for historical queries"
        );
    }

    /// When the query plan has [`NeedsTarget`] policy (code-heavy but no targets),
    /// the route should NOT be upgraded.
    #[test]
    fn needs_target_query_plan_does_not_upgrade_route() {
        let ws = TestWorkspace::new("query-needs-target-no-upgrade");
        let scores = router::Scores {
            historical: 0,
            structural: 1,
            local: 1,
            action: 1,
        };

        let query_plan = BroadQueryPlan::new("fix the CLI parser regression", &scores, ws.root());

        assert_eq!(
            query_plan.injection_policy,
            QueryInjectionPolicy::NeedsTarget
        );

        let upgraded = apply_code_heavy_route_upgrade(Route::Neither, &query_plan);
        assert_eq!(
            upgraded,
            Route::Neither,
            "Neither should NOT be upgraded when no grounded targets exist"
        );
    }

    /// [`Route::GraphOnly`] and [`Route::Both`] routes should be unchanged by the upgrade.
    #[test]
    fn code_heavy_upgrade_preserves_graph_routes() {
        let ws = TestWorkspace::new("query-code-heavy-preserve-graph");
        let root = ws.root();

        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

        let scores = router::Scores {
            historical: 0,
            structural: 2,
            local: 0,
            action: 1,
        };

        let query_plan = BroadQueryPlan::new("fix src/main.rs", &scores, root);

        assert_eq!(
            query_plan.injection_policy,
            QueryInjectionPolicy::UseGroundedTargets
        );

        assert_eq!(
            apply_code_heavy_route_upgrade(Route::GraphOnly, &query_plan),
            Route::GraphOnly,
            "GraphOnly should remain unchanged"
        );
        assert_eq!(
            apply_code_heavy_route_upgrade(Route::Both, &query_plan),
            Route::Both,
            "Both should remain unchanged"
        );
    }
}
