use anyhow::Result;
use serde_json::{Value, json};

use crate::memory::{format_memory_hit, synthesize_memory_brief};
use crate::types::{ImpactSummary, MemoryBrief, MemoryHit, RouteDecision};

fn architecture_summary_lines(memory_brief: &MemoryBrief, graph_hits: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(item) = memory_brief.decisions.first() {
        lines.push(item.clone());
    }
    if let Some(item) = memory_brief.constraints.first() {
        lines.push(item.clone());
    }
    if let Some(item) = graph_hits.first() {
        lines.push(format!("Structure: {}", item));
    }
    if let Some(item) = memory_brief.status.first() {
        lines.push(item.clone());
    }
    lines.truncate(4);
    lines
}

fn push_section(lines: &mut Vec<String>, items: &[String]) {
    for item in items {
        lines.push(format!("  - {}", item));
    }
}

fn structural_context_lines(memory_hits: &[MemoryHit]) -> Vec<String> {
    let Some(impact) = memory_hits.iter().find_map(|hit| {
        hit.graph_context
            .as_ref()
            .and_then(|context| context.impact_summary.as_ref())
    }) else {
        return vec![];
    };

    vec![format_impact_summary(impact)]
}

fn format_impact_summary(impact: &ImpactSummary) -> String {
    let targets = if impact.target_symbols.is_empty() {
        "unknown targets".to_string()
    } else {
        impact.target_symbols.join(", ")
    };
    let processes = if impact.affected_processes.is_empty() {
        "no named processes".to_string()
    } else {
        impact.affected_processes.join(", ")
    };
    format!(
        "Structural context: {} -> d1={}, d2={}, d3={}, risk={}, processes={}.",
        targets,
        impact.blast_radius.direct,
        impact.blast_radius.indirect,
        impact.blast_radius.transitive,
        impact.risk_level,
        processes
    )
}

fn trim_tail(lines: &mut Vec<String>, heading: &str, minimum_kept: usize) -> bool {
    let Some(start) = lines.iter().position(|line| line == heading) else {
        return false;
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| {
            if !line.starts_with("  - ") && line.ends_with(':') {
                Some(index)
            } else {
                None
            }
        })
        .unwrap_or(lines.len());
    let items = end.saturating_sub(start + 1);
    if items <= minimum_kept {
        return false;
    }
    lines.remove(end - 1);
    true
}

fn enforce_word_budget(lines: &mut Vec<String>, word_limit: usize) -> bool {
    let mut truncated = false;
    loop {
        let current = lines.join("\n");
        if current.split_whitespace().count() <= word_limit {
            break;
        }
        let trimmed = trim_tail(lines, "- Memory Sources:", 1)
            || trim_tail(lines, "- Graph:", 1)
            || trim_tail(lines, "- Memory Brief:", 4)
            || trim_tail(lines, "Architecture Summary:", 2)
            || trim_tail(lines, "Open Uncertainty:", 1);
        if !trimmed {
            break;
        }
        truncated = true;
    }
    if lines.join("\n").split_whitespace().count() > word_limit {
        let mut words = Vec::new();
        for line in lines.iter() {
            let line_words = line.split_whitespace().collect::<Vec<_>>();
            if words.len() + line_words.len() > word_limit.saturating_sub(8) {
                break;
            }
            words.extend(line_words);
        }
        let truncated = format!(
            "{}\n- Context truncated to stay within the retrieval word budget.\n</layers_context>",
            words
                .join(" ")
                .replace(" </layers_context>", "")
                .replace("<layers_context> ", "<layers_context>\n")
        );
        *lines = truncated.lines().map(|line| line.to_string()).collect();
        return true;
    }
    truncated
}

pub fn build_context(
    query: &str,
    decision: &RouteDecision,
    memory_hits: &[MemoryHit],
    memory_issue: Option<&str>,
    graph_hits: &[String],
    graph_issue: Option<&str>,
) -> Result<Value> {
    let memory_brief = synthesize_memory_brief(memory_hits);
    let architecture_summary = architecture_summary_lines(&memory_brief, graph_hits);
    let structural_context = structural_context_lines(memory_hits);
    let mut why_retrieved = Vec::new();
    let mut why_not = Vec::new();
    let mut uncertainty = Vec::new();
    if decision.route == "memory_only" || decision.route == "both" {
        if !memory_hits.is_empty() {
            why_retrieved.push("Memory: prior decisions or historical context appear decision-critical. Semantic retrieval is preferred when available.".to_string());
        } else {
            why_not.push("Memory: route requested history, but no relevant council records matched the query.".to_string());
            if let Some(issue) = memory_issue {
                uncertainty.push(issue.to_string());
            } else {
                uncertainty.push(
                    "Historical retrieval returned no relevant local council records.".to_string(),
                );
            }
        }
    } else {
        why_not.push("Memory: route did not justify historical retrieval.".to_string());
    }
    if decision.route == "graph_only" || decision.route == "both" {
        if !graph_hits.is_empty() {
            why_retrieved.push(
                "Graph: repo structure appears relevant to execution or impact analysis."
                    .to_string(),
            );
        } else {
            let issue = graph_issue.unwrap_or("GitNexus retrieval unavailable.");
            why_not.push(format!(
                "Graph: route requested structure, but GitNexus was unavailable. {}",
                issue
            ));
            uncertainty.push(issue.to_string());
        }
    } else {
        why_not.push("Graph: route did not justify structural retrieval.".to_string());
    }
    if decision.route == "neither" {
        uncertainty.push(
            "Layers intentionally refused retrieval because local evidence likely suffices."
                .to_string(),
        );
    }

    let mut lines = vec![
        "<layers_context>".to_string(),
        format!("Route: {}", decision.route),
        format!("Confidence: {}", decision.confidence),
        "Architecture Summary:".to_string(),
    ];
    if architecture_summary.is_empty() {
        lines.push("- None".to_string());
    } else {
        for item in &architecture_summary {
            lines.push(format!("- {}", item));
        }
    }
    lines.extend(["Why Retrieved:".to_string()]);
    if why_retrieved.is_empty() {
        lines.push("- None".to_string());
    } else {
        for item in &why_retrieved {
            lines.push(format!("- {}", item));
        }
    }
    lines.push("Why Not Retrieved:".to_string());
    for item in &why_not {
        lines.push(format!("- {}", item));
    }
    lines.push("Evidence:".to_string());
    if !memory_hits.is_empty() {
        let has_brief = !memory_brief.decisions.is_empty()
            || !memory_brief.constraints.is_empty()
            || !memory_brief.status.is_empty()
            || !memory_brief.next_steps.is_empty()
            || !memory_brief.postmortems.is_empty()
            || !memory_brief.notable_context.is_empty();
        if has_brief {
            lines.push("- Memory Brief:".to_string());
            push_section(&mut lines, &memory_brief.decisions);
            push_section(&mut lines, &memory_brief.constraints);
            push_section(&mut lines, &memory_brief.status);
            push_section(&mut lines, &memory_brief.next_steps);
            push_section(&mut lines, &memory_brief.postmortems);
            push_section(&mut lines, &memory_brief.notable_context);
        }
        lines.push("- Memory Sources:".to_string());
        for hit in memory_hits {
            lines.push(format!("  - {}", format_memory_hit(hit)));
        }
    }
    if !graph_hits.is_empty() {
        lines.push("- Graph:".to_string());
        for fact in graph_hits {
            lines.push(format!("  - {}", fact));
        }
    }
    if !structural_context.is_empty() {
        lines.push("- Structural Context:".to_string());
        push_section(&mut lines, &structural_context);
    }
    if memory_hits.is_empty() && graph_hits.is_empty() {
        lines.push("- None".to_string());
    }
    lines.push("Open Uncertainty:".to_string());
    if uncertainty.is_empty() {
        lines.push("- None".to_string());
    } else {
        for item in &uncertainty {
            lines.push(format!("- {}", item));
        }
    }
    lines.push("</layers_context>".to_string());
    if lines.join("\n").split_whitespace().count() > 1200 {
        if enforce_word_budget(&mut lines, 1200) {
            uncertainty.push(
                "Retrieved evidence was truncated to stay within the 1200-word budget.".to_string(),
            );
        }
        lines.retain(|line| !line.is_empty() || line == "</layers_context>");
    }
    let context_text = lines.join("\n");

    Ok(json!({
        "query": query,
        "route": decision.route,
        "confidence": decision.confidence,
        "scores": decision.scores,
        "matches": decision.matches,
        "rationale": decision.rationale,
        "why_retrieved": why_retrieved,
        "why_not_retrieved": why_not,
        "architecture_summary": architecture_summary,
        "structural_context": structural_context,
        "memory_brief": memory_brief,
        "evidence": {
            "memory": memory_hits,
            "graph": graph_hits,
        },
        "open_uncertainty": uncertainty,
        "context_text": context_text,
    }))
}
