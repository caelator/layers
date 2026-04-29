//! Deterministic quality reports for context packet injection decisions.

use crate::{ContextPacket, TaskSpec};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Agent-facing policy recommendation for a packet.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InjectionRecommendation {
    InjectFull,
    InjectCompact,
    Abstain,
    NeedsTarget,
}

/// Bounded packet quality scores. Each field is in `0..=5`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PacketQualityScores {
    pub relevance: u8,
    pub completeness: u8,
    pub specificity: u8,
    pub freshness: u8,
    pub grounding: u8,
    pub concision: u8,
    pub noise_absence: u8,
}

impl PacketQualityScores {
    #[must_use]
    pub const fn high() -> Self {
        Self {
            relevance: 5,
            completeness: 5,
            specificity: 5,
            freshness: 5,
            grounding: 5,
            concision: 5,
            noise_absence: 5,
        }
    }

    #[must_use]
    pub fn average(&self) -> f64 {
        f64::from(
            self.relevance
                + self.completeness
                + self.specificity
                + self.freshness
                + self.grounding
                + self.concision
                + self.noise_absence,
        ) / 7.0
    }
}

/// Deterministic report for a packet graded against a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PacketQualityReport {
    pub scores: PacketQualityScores,
    pub target_coverage_ratio: f64,
    pub validation_coverage_ratio: f64,
    pub warning_penalty: f64,
    pub missed_critical_context: bool,
    pub hallucinated_or_stale_context: bool,
    pub recommendation: InjectionRecommendation,
    pub reasons: Vec<String>,
}

impl PacketQualityReport {
    /// Grade a packet against a task using deterministic local signals only.
    #[must_use]
    pub fn grade(packet: &ContextPacket, task: &TaskSpec) -> Self {
        let text = packet_text(packet);
        let target_coverage_ratio = coverage_ratio(&expected_targets(task), packet, &text);
        let validation_coverage_ratio = string_coverage(&task.expected_validation_commands, &text);
        let warning_penalty = warning_penalty(packet);
        let hallucinated_or_stale_context = packet.warnings.iter().any(|warning| {
            let code = warning.code.to_ascii_lowercase();
            let message = warning.message.to_ascii_lowercase();
            code.contains("stale")
                || code.contains("hallucinat")
                || message.contains("stale")
                || message.contains("hallucinat")
        });
        let memory_only = source_kinds(packet)
            .iter()
            .all(|kind| kind.contains("memory") || kind.contains("uc") || kind.contains("keyword"));
        let low_confidence = packet.low_confidence_fallback
            || packet.confidence.eq_ignore_ascii_case("low")
            || packet.retrieval.fallback_reason.is_some();

        let code_heavy = task.category.is_code_heavy() || looks_code_heavy(&task.prompt);
        let expected_target_count = expected_targets(task).len();
        let missed_critical_context = code_heavy
            && expected_target_count > 0
            && target_coverage_ratio < 0.5
            && !task.negative_control;

        let mut reasons = Vec::new();
        if task.negative_control {
            reasons.push("task is a negative control; useful behavior is to abstain unless context is clearly necessary".to_string());
        }
        if missed_critical_context {
            reasons.push("packet missed expected relevant code targets".to_string());
        }
        if low_confidence {
            reasons.push("packet is low-confidence or fallback-derived".to_string());
        }
        if memory_only && code_heavy {
            reasons.push("code-heavy task received memory-only context".to_string());
        }
        if hallucinated_or_stale_context {
            reasons.push("packet contains stale or hallucination warnings".to_string());
        }

        let item_count = packet
            .sections
            .iter()
            .map(|section| section.items.len())
            .sum::<usize>();
        let scores = PacketQualityScores {
            relevance: score_from_ratio(if task.negative_control {
                1.0
            } else {
                target_coverage_ratio.max(keyword_overlap(&task.prompt, &text))
            }),
            completeness: if expected_target_count == 0 && !code_heavy {
                3
            } else {
                score_from_ratio(target_coverage_ratio)
            },
            specificity: specificity_score(packet, code_heavy),
            freshness: if hallucinated_or_stale_context {
                1
            } else {
                5u8.saturating_sub(warning_penalty.round() as u8).max(1)
            },
            grounding: grounding_score(packet),
            concision: concision_score(packet),
            noise_absence: 5u8.saturating_sub(warning_penalty.round() as u8).max(1),
        };

        let recommendation = if task.negative_control && item_count <= 2 {
            InjectionRecommendation::Abstain
        } else if code_heavy
            && expected_target_count == 0
            && packet.sections.iter().all(|s| s.id != "code")
        {
            InjectionRecommendation::NeedsTarget
        } else if missed_critical_context || (code_heavy && memory_only && low_confidence) {
            InjectionRecommendation::Abstain
        } else if hallucinated_or_stale_context || warning_penalty >= 2.0 || packet.budget.truncated
        {
            InjectionRecommendation::InjectCompact
        } else if scores.average() >= 4.0 && target_coverage_ratio >= 0.75 {
            InjectionRecommendation::InjectFull
        } else if scores.average() >= 3.0 {
            InjectionRecommendation::InjectCompact
        } else {
            InjectionRecommendation::Abstain
        };

        if reasons.is_empty() {
            reasons.push(
                match recommendation {
                    InjectionRecommendation::InjectFull => {
                        "packet has strong target coverage and low warning burden"
                    }
                    InjectionRecommendation::InjectCompact => {
                        "packet has usable context but should be compacted or verified carefully"
                    }
                    InjectionRecommendation::Abstain => {
                        "packet has low expected value for this task"
                    }
                    InjectionRecommendation::NeedsTarget => {
                        "task needs explicit targets before context injection"
                    }
                }
                .to_string(),
            );
        }

        Self {
            scores,
            target_coverage_ratio,
            validation_coverage_ratio,
            warning_penalty,
            missed_critical_context,
            hallucinated_or_stale_context,
            recommendation,
            reasons,
        }
    }
}

fn packet_text(packet: &ContextPacket) -> String {
    let mut text = format!(
        "{} {} {} {}",
        packet.query, packet.route, packet.confidence, packet.evidence
    );
    for section in &packet.sections {
        text.push(' ');
        text.push_str(&section.id);
        text.push(' ');
        text.push_str(&section.title);
        if let Some(summary) = &section.summary {
            text.push(' ');
            text.push_str(summary);
        }
        for item in &section.items {
            text.push(' ');
            text.push_str(&item.title);
            text.push(' ');
            text.push_str(&item.body);
            text.push(' ');
            text.push_str(&item.source.kind);
            text.push(' ');
            text.push_str(&item.source.uri);
            if let Some(path) = &item.source.repo_path {
                text.push(' ');
                text.push_str(path);
            }
            text.push(' ');
            text.push_str(&item.selected_reason);
        }
    }
    text.to_ascii_lowercase()
}

fn expected_targets(task: &TaskSpec) -> Vec<String> {
    task.expected_relevant_files
        .iter()
        .chain(task.target_files.iter())
        .map(|path| normalize_path(path))
        .chain(
            task.target_symbols
                .iter()
                .map(|symbol| symbol.to_ascii_lowercase()),
        )
        .collect()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn coverage_ratio(expected: &[String], packet: &ContextPacket, text: &str) -> f64 {
    if expected.is_empty() {
        return if packet.sections.is_empty() { 0.0 } else { 1.0 };
    }
    let hit_count = expected
        .iter()
        .filter(|needle| text.contains(needle.as_str()))
        .count();
    hit_count as f64 / expected.len() as f64
}

fn string_coverage(expected: &[String], text: &str) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    let hit_count = expected
        .iter()
        .filter(|needle| text.contains(&needle.to_ascii_lowercase()))
        .count();
    hit_count as f64 / expected.len() as f64
}

fn warning_penalty(packet: &ContextPacket) -> f64 {
    let warning_score = packet
        .warnings
        .iter()
        .map(|warning| match warning.severity.as_str() {
            "error" => 1.5,
            "warning" => 1.0,
            _ => 0.25,
        })
        .sum::<f64>();
    warning_score
        + f64::from(packet.low_confidence_fallback)
        + f64::from(packet.budget.truncated)
        + f64::from(packet.retrieval.fallback_reason.is_some())
}

fn source_kinds(packet: &ContextPacket) -> Vec<String> {
    packet
        .sections
        .iter()
        .flat_map(|section| section.items.iter())
        .map(|item| item.source.kind.to_ascii_lowercase())
        .collect()
}

fn score_from_ratio(ratio: f64) -> u8 {
    (ratio.clamp(0.0, 1.0) * 5.0).round() as u8
}

fn specificity_score(packet: &ContextPacket, code_heavy: bool) -> u8 {
    if packet.sections.iter().any(|section| section.id == "code") {
        5
    } else if code_heavy {
        2
    } else {
        3
    }
}

fn grounding_score(packet: &ContextPacket) -> u8 {
    let items = packet
        .sections
        .iter()
        .flat_map(|section| section.items.iter())
        .collect::<Vec<_>>();
    if items.is_empty() {
        return 0;
    }
    let cited = items
        .iter()
        .filter(|item| !item.source.kind.trim().is_empty() && !item.source.uri.trim().is_empty())
        .count();
    score_from_ratio(cited as f64 / items.len() as f64)
}

fn concision_score(packet: &ContextPacket) -> u8 {
    if packet.budget.truncated
        || (packet.budget.max_units > 0 && packet.budget.used_units > packet.budget.max_units)
    {
        2
    } else if packet.budget.used_units > 2_500 {
        3
    } else {
        5
    }
}

fn keyword_overlap(prompt: &str, text: &str) -> f64 {
    let keywords = prompt
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .map(str::to_ascii_lowercase)
        .filter(|word| word.len() >= 4)
        .collect::<Vec<_>>();
    if keywords.is_empty() {
        return 0.0;
    }
    let hits = keywords
        .iter()
        .filter(|word| text.contains(word.as_str()))
        .count();
    hits as f64 / keywords.len() as f64
}

fn looks_code_heavy(prompt: &str) -> bool {
    let prompt = prompt.to_ascii_lowercase();
    [
        "fix",
        "implement",
        "refactor",
        "test",
        "compile",
        "clippy",
        "cargo",
        ".rs",
        "src/",
    ]
    .iter()
    .any(|needle| prompt.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ContextBudget, ContextItem, ContextPacket, ContextSection, ContextSource, ContextWarning,
        TaskCategory,
    };
    use chrono::Utc;
    use std::path::PathBuf;

    #[test]
    fn high_quality_report_recommends_inject_full() {
        let packet = packet_with_file("src/main.rs", "cargo test -q packet_quality");
        let task = code_task();

        let report = PacketQualityReport::grade(&packet, &task);

        assert_eq!(report.recommendation, InjectionRecommendation::InjectFull);
        assert_eq!(report.target_coverage_ratio, 1.0);
    }

    #[test]
    fn low_confidence_memory_only_code_packet_recommends_abstain() {
        let mut packet =
            ContextPacket::new("p".into(), "w".into(), "fix src/main.rs".into(), Utc::now());
        packet.confidence = "low".to_string();
        packet.low_confidence_fallback = true;
        packet.sections.push(ContextSection {
            id: "memory".to_string(),
            title: "Memory".to_string(),
            summary: None,
            items: vec![ContextItem::cited(
                "m1",
                "Memory",
                "A generic note",
                ContextSource::new("memory", "memory.jsonl"),
                "keyword match",
            )],
        });

        let report = PacketQualityReport::grade(&packet, &code_task());

        assert_eq!(report.recommendation, InjectionRecommendation::Abstain);
        assert!(
            report
                .reasons
                .iter()
                .any(|reason| reason.contains("memory-only"))
        );
    }

    #[test]
    fn missing_target_files_marks_missed_critical_context() {
        let packet = packet_with_file("src/lib.rs", "cargo test");

        let report = PacketQualityReport::grade(&packet, &code_task());

        assert!(report.missed_critical_context);
        assert_eq!(report.recommendation, InjectionRecommendation::Abstain);
    }

    #[test]
    fn warning_heavy_packet_receives_warning_penalty() {
        let mut packet = packet_with_file("src/main.rs", "cargo test");
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "stale_context".to_string(),
            message: "stale source".to_string(),
        });

        let report = PacketQualityReport::grade(&packet, &code_task());

        assert!(report.warning_penalty >= 1.0);
        assert!(report.hallucinated_or_stale_context);
        assert_eq!(
            report.recommendation,
            InjectionRecommendation::InjectCompact
        );
    }

    #[test]
    fn negative_control_with_little_context_recommends_abstain() {
        let packet = ContextPacket::new("p".into(), "w".into(), "what is 2+2".into(), Utc::now());
        let task = TaskSpec {
            task_id: "n".to_string(),
            title: "Negative".to_string(),
            prompt: "What is 2+2?".to_string(),
            category: TaskCategory::NegativeControl,
            repo_root: None,
            target_files: Vec::new(),
            target_symbols: Vec::new(),
            expected_relevant_files: Vec::new(),
            expected_validation_commands: Vec::new(),
            negative_control: true,
            success_rubric: crate::SuccessRubric::default(),
        };

        let report = PacketQualityReport::grade(&packet, &task);

        assert_eq!(report.recommendation, InjectionRecommendation::Abstain);
    }

    fn code_task() -> TaskSpec {
        TaskSpec {
            task_id: "code".to_string(),
            title: "Code".to_string(),
            prompt: "Fix src/main.rs and run cargo test".to_string(),
            category: TaskCategory::Bugfix,
            repo_root: None,
            target_files: vec![PathBuf::from("src/main.rs")],
            target_symbols: Vec::new(),
            expected_relevant_files: vec![PathBuf::from("src/main.rs")],
            expected_validation_commands: vec!["cargo test".to_string()],
            negative_control: false,
            success_rubric: crate::SuccessRubric::default(),
        }
    }

    fn packet_with_file(path: &str, validation: &str) -> ContextPacket {
        let mut packet = ContextPacket::new("p".into(), "w".into(), "fix".into(), Utc::now());
        packet.confidence = "high".to_string();
        packet.budget = ContextBudget {
            max_units: 1_000,
            used_units: 100,
            unit: "words".to_string(),
            truncated: false,
        };
        packet.sections.push(ContextSection {
            id: "code".to_string(),
            title: "Code".to_string(),
            summary: None,
            items: vec![ContextItem::cited(
                "c1",
                path,
                format!("Relevant code in {path}"),
                ContextSource::new("file", path).with_repo_path(path),
                "target file",
            )],
        });
        packet.sections.push(ContextSection {
            id: "validation".to_string(),
            title: "Validation".to_string(),
            summary: None,
            items: vec![ContextItem::cited(
                "v1",
                validation,
                validation,
                ContextSource::new("validation", "policy"),
                "verify",
            )],
        });
        packet
    }
}
