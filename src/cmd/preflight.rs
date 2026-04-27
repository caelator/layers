//! Automatic local research for task-specific context packets.

use anyhow::{Context, Result, anyhow};
use layers_core::{ContextBudget, ContextItem, ContextPacket, ContextWarning, RetrievalReport};
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::cmd::autoresearch::{AutoresearchPacketBridgeOptions, add_autoresearch_to_packet};
use crate::config::{canonical_curated_memory_path, workspace_root};
use crate::context_packet_compiler::{
    add_workspace_section, cited_item, code_impact_section, collect_workspace_state,
    context_section, finalize_packet, git_output, repo_source, workspace_id,
};

const DEFAULT_BUDGET_WORDS: usize = 1_800;
const MAX_MEMORY_ITEMS: usize = 5;
const MAX_CODE_ITEMS: usize = 8;

/// Arguments for the `layers preflight` command.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct PreflightArgs {
    /// Task or question to research before editing.
    pub task: String,
    /// Optional user-provided files, directories, symbols, or tests.
    pub targets: Vec<String>,
    /// Output structured JSON.
    pub json: bool,
    /// Output an agent-ready prompt.
    pub agent_prompt: bool,
    /// Skip audit side effects. Reserved for parity with query workflows.
    pub no_audit: bool,
    /// Fail if minimum context coverage is not met.
    pub strict: bool,
}

/// Run local preflight and print the requested packet format.
pub fn handle_preflight(args: &PreflightArgs) -> Result<()> {
    let packet = build_preflight_packet(args)?;
    if args.strict && !minimum_bar_passes(&packet) {
        return Err(anyhow!(
            "preflight did not meet the minimum context bar; rerun without --strict to inspect warnings"
        ));
    }
    if args.agent_prompt {
        println!("{}", packet.to_agent_prompt());
    } else if args.json {
        println!("{}", serde_json::to_string_pretty(&packet)?);
    } else {
        println!("{}", packet.to_markdown());
    }
    Ok(())
}

fn build_preflight_packet(args: &PreflightArgs) -> Result<ContextPacket> {
    let workspace = workspace_root();
    let workspace_id = workspace_id(&workspace);
    let mut packet = ContextPacket::new(
        format!("preflight-{}", uuid::Uuid::new_v4()),
        workspace_id,
        args.task.clone(),
        chrono::Utc::now(),
    );
    packet.task.clone_from(&args.task);
    packet.route = "preflight".to_string();
    packet.budget = ContextBudget {
        max_units: DEFAULT_BUDGET_WORDS,
        used_units: 0,
        unit: "words".to_string(),
        truncated: false,
    };

    let explicit_targets = args.targets.clone();
    let inferred_targets = infer_targets(&args.task, &explicit_targets, &workspace);
    let code_heavy = is_code_heavy_task(&args.task, &inferred_targets);
    let workspace_state = collect_workspace_state(&workspace);
    packet.git_ref.clone_from(&workspace_state.head);

    add_workspace_section(&mut packet, &workspace_state);
    add_memory_section(&mut packet, &args.task);
    let autoresearch_findings =
        add_autoresearch_section(&mut packet, &args.task, &inferred_targets);
    add_code_section(&mut packet, &workspace, &inferred_targets, code_heavy);
    add_validation_section(&mut packet, code_heavy, &inferred_targets);
    add_preflight_summary_section(&mut packet, args, code_heavy, &inferred_targets);

    packet.retrieval = RetrievalReport {
        memory_source: "curated-memory-jsonl".to_string(),
        memory_latency_ms: 0,
        graph_latency_ms: 0,
        fallback_reason: Some(
            "preflight v1 uses local workspace, memory, and file/Git fallback collectors"
                .to_string(),
        ),
    };
    packet.retrieval_meta = packet.retrieval.clone();
    packet.scores = json!({
        "code_heavy": code_heavy,
        "targets": inferred_targets.len(),
        "autoresearch_findings": autoresearch_findings,
        "workspace_dirty": workspace_state.dirty,
    });
    packet.why_retrieved = "Preflight compiles local pre-edit context from workspace state, persisted autoresearch findings, memory, code targets, and validation policy.".to_string();
    packet.why_not_retrieved = String::new();
    packet.confidence = ResearchQuality::new(
        code_heavy,
        has_section(&packet, "memory"),
        has_section(&packet, "code"),
        has_warning(&packet, "low_memory_relevance"),
    )
    .confidence;
    packet.budget.used_units = estimate_packet_words(&packet);
    if packet.budget.used_units > packet.budget.max_units {
        packet.budget.truncated = true;
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "budget_may_be_exceeded".to_string(),
            message: format!(
                "Preflight packet is estimated at {} words, above the {} word budget.",
                packet.budget.used_units, packet.budget.max_units
            ),
        });
    }
    if !args.no_audit {
        packet.warnings.push(ContextWarning {
            severity: "info".to_string(),
            code: "audit_not_yet_persisted".to_string(),
            message: "Preflight audit persistence is not enabled in this beta implementation."
                .to_string(),
        });
    }
    finalize_packet(&mut packet, true);
    Ok(packet)
}

fn has_section(packet: &ContextPacket, section_id: &str) -> bool {
    packet
        .sections
        .iter()
        .any(|section| section.id == section_id)
}

fn has_warning(packet: &ContextPacket, code: &str) -> bool {
    packet.warnings.iter().any(|warning| warning.code == code)
}

fn minimum_bar_passes(packet: &ContextPacket) -> bool {
    has_section(packet, "workspace")
        && has_section(packet, "memory")
        && has_section(packet, "validation")
        && (!is_packet_code_heavy(packet) || has_section(packet, "code"))
}

fn is_packet_code_heavy(packet: &ContextPacket) -> bool {
    packet
        .scores
        .get("code_heavy")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn add_preflight_summary_section(
    packet: &mut ContextPacket,
    args: &PreflightArgs,
    code_heavy: bool,
    targets: &[String],
) {
    let body = format!(
        "Preflight classified this task as {}. Explicit targets: {}. Effective targets: {}.",
        if code_heavy {
            "code-heavy"
        } else {
            "memory-oriented"
        },
        if args.targets.is_empty() {
            "none".to_string()
        } else {
            args.targets.join(", ")
        },
        if targets.is_empty() {
            "none".to_string()
        } else {
            targets.join(", ")
        }
    );
    packet.sections.insert(
        0,
        context_section(
            "preflight_summary",
            "Preflight Summary",
            "Preflight planning result.",
            vec![context_item(
                "research-summary-1",
                "Preflight classification",
                &body,
                "preflight",
                "local-planner",
                None,
                "summarizes the local preflight plan before source collection",
                vec!["planning".to_string()],
            )],
        ),
    );
}

fn add_memory_section(packet: &mut ContextPacket, task: &str) {
    let memory_path = canonical_curated_memory_path();
    let Ok(contents) = std::fs::read_to_string(&memory_path) else {
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "memory_unavailable".to_string(),
            message: format!(
                "Could not read curated memory at {}.",
                memory_path.display()
            ),
        });
        return;
    };
    let keywords = keywords(task);
    let mut matches = contents
        .lines()
        .filter(|line| keyword_score(line, &keywords) > 0)
        .take(MAX_MEMORY_ITEMS)
        .enumerate()
        .map(|(idx, line)| {
            context_item(
                &format!("memory-{}", idx + 1),
                "Curated memory match",
                line,
                "memory",
                &memory_path.display().to_string(),
                None,
                "curated memory shares terms with the preflight task",
                vec!["memory".to_string()],
            )
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        packet.warnings.push(ContextWarning {
            severity: "warning".to_string(),
            code: "low_memory_relevance".to_string(),
            message: "Curated memory produced no keyword-relevant preflight matches.".to_string(),
        });
        matches.push(context_item(
            "memory-empty-1",
            "No relevant curated memory found",
            "Preflight searched curated memory but found no keyword-relevant records.",
            "memory",
            &memory_path.display().to_string(),
            None,
            "records memory-source coverage and explains reduced confidence",
            vec!["memory".to_string(), "degraded".to_string()],
        ));
    }
    packet.sections.push(context_section(
        "memory",
        "Project Memory",
        "Relevant explicit project memory or memory-source warning.",
        matches,
    ));
}

fn add_autoresearch_section(packet: &mut ContextPacket, task: &str, targets: &[String]) -> usize {
    add_autoresearch_to_packet(
        packet,
        AutoresearchPacketBridgeOptions {
            task,
            targets,
            limit: MAX_MEMORY_ITEMS,
            unavailable_message: "No persisted autoresearch store was available for preflight.",
        },
    )
}

fn add_code_section(
    packet: &mut ContextPacket,
    workspace: &Path,
    targets: &[String],
    code_heavy: bool,
) {
    let mut items = Vec::new();
    for target in targets.iter().take(MAX_CODE_ITEMS) {
        let path = workspace.join(target);
        if path.is_file() {
            if let Ok(body) = read_file_excerpt(&path) {
                items.push(context_item(
                    &format!("code-{}", items.len() + 1),
                    target,
                    &body,
                    "file",
                    target,
                    Some(target.clone()),
                    "explicit or inferred target file should be inspected before editing",
                    vec!["code".to_string()],
                ));
            }
        } else if path.is_dir() {
            items.push(context_item(
                &format!("code-{}", items.len() + 1),
                target,
                &summarize_directory(&path),
                "directory",
                target,
                Some(target.clone()),
                "explicit or inferred target directory scopes the implementation",
                vec!["code".to_string()],
            ));
        }
    }
    if items.is_empty() && !targets.is_empty() {
        let target_keywords = targets
            .iter()
            .flat_map(|target| keywords(target))
            .collect::<Vec<_>>();
        items.extend(fallback_code_search(workspace, &target_keywords));
    }
    if items.is_empty() && code_heavy {
        let fallback = fallback_code_search(workspace, &keywords(&packet.query));
        items.extend(fallback);
    }
    if items.is_empty() {
        if code_heavy {
            packet.warnings.push(ContextWarning {
                severity: "warning".to_string(),
                code: "code_context_unavailable".to_string(),
                message: "Preflight classified this as code-heavy but found no target files or fallback code hits.".to_string(),
            });
        }
        return;
    }
    packet.sections.push(code_impact_section(items));
}

fn add_validation_section(packet: &mut ContextPacket, code_heavy: bool, targets: &[String]) {
    let commands = infer_validation_commands(code_heavy, targets);
    let items = commands
        .iter()
        .enumerate()
        .map(|(idx, command)| {
            context_item(
                &format!("validation-{}", idx + 1),
                &command.command,
                &format!(
                    "Reason: {}\nRequired: {}\nExpected signal: {}",
                    command.reason, command.required, command.expected_signal
                ),
                "validation",
                "preflight-validation-policy",
                None,
                "validation commands prevent context-only research from becoming unverified implementation",
                vec!["validation".to_string()],
            )
        })
        .collect();
    packet.sections.push(context_section(
        "validation",
        "Suggested Validation Commands",
        "Commands an agent should run after using this research.",
        items,
    ));
}

fn context_item(
    id: &str,
    title: &str,
    body: &str,
    source_kind: &str,
    source_uri: &str,
    repo_path: Option<String>,
    selected_reason: &str,
    tags: Vec<String>,
) -> ContextItem {
    cited_item(
        id,
        title,
        body,
        repo_source(source_kind, source_uri, repo_path),
        selected_reason,
        tags,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidationCommand {
    command: String,
    reason: String,
    required: bool,
    expected_signal: String,
}

fn infer_validation_commands(code_heavy: bool, targets: &[String]) -> Vec<ValidationCommand> {
    let mut commands = vec![ValidationCommand {
        command: "git diff --check".to_string(),
        reason: "detect whitespace and patch hygiene issues before handoff".to_string(),
        required: true,
        expected_signal: "no diff formatting errors".to_string(),
    }];
    if code_heavy {
        commands.push(ValidationCommand {
            command: "cargo test --workspace --all-targets".to_string(),
            reason: "verify all Rust crates and binary targets after code-heavy changes"
                .to_string(),
            required: true,
            expected_signal: "all tests pass".to_string(),
        });
        commands.push(ValidationCommand {
            command: "cargo clippy --workspace --all-targets -- -D warnings".to_string(),
            reason: "production-readiness gate for Rust lint debt".to_string(),
            required: true,
            expected_signal: "no clippy warnings or documented baseline debt".to_string(),
        });
    }
    if targets
        .iter()
        .any(|target| target.contains("context_packet") || target.contains("query"))
    {
        commands.push(ValidationCommand {
            command: "./target/debug/layers query --no-audit --json \"What should I know before editing src/cmd/query.rs?\" | python3 -m json.tool >/dev/null".to_string(),
            reason: "ContextPacket and query changes must keep machine-readable JSON valid".to_string(),
            required: true,
            expected_signal: "JSON parses successfully".to_string(),
        });
    }
    commands
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResearchQuality {
    confidence: String,
    passes_minimum_bar: bool,
}

impl ResearchQuality {
    #[allow(clippy::fn_params_excessive_bools)]
    fn new(code_heavy: bool, has_memory: bool, has_code: bool, low_memory_relevance: bool) -> Self {
        let passes_minimum_bar = !code_heavy || has_code;
        let confidence = if (code_heavy || low_memory_relevance) && !has_code {
            "low"
        } else if low_memory_relevance {
            "medium"
        } else if has_memory && has_code {
            "high"
        } else {
            "medium"
        };
        Self {
            confidence: confidence.to_string(),
            passes_minimum_bar,
        }
    }
}

fn is_code_heavy_task(task: &str, targets: &[String]) -> bool {
    let lower = task.to_lowercase();
    let code_terms = [
        "code",
        "clippy",
        "test",
        "tests",
        "rust",
        "refactor",
        "build",
        "mcp",
        "validate",
        "implementation",
        "feature",
        "function",
        "module",
        "crate",
        "preflight",
    ];
    targets.iter().any(|target| looks_like_path(target))
        || code_terms.iter().any(|term| lower.contains(term))
}

fn infer_targets(task: &str, explicit_targets: &[String], workspace: &Path) -> Vec<String> {
    let mut targets = explicit_targets.to_vec();
    for token in task.split_whitespace() {
        let cleaned = token.trim_matches(|ch: char| {
            matches!(ch, ',' | '.' | ':' | ';' | ')' | '(' | '`' | '"' | '\'')
        });
        if looks_like_path(cleaned) && !targets.iter().any(|target| target == cleaned) {
            targets.push(cleaned.to_string());
        }
    }
    if targets.is_empty() && task.to_lowercase().contains("preflight") {
        for fallback in ["src/cmd/preflight.rs", "src/main.rs", "src/cmd/mod.rs"] {
            if workspace.join(fallback).exists() || fallback == "src/cmd/preflight.rs" {
                targets.push(fallback.to_string());
            }
        }
    }
    targets
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/')
        || has_source_extension(value)
        || value.starts_with("crates/")
        || value.starts_with("src/")
        || value.starts_with("docs/")
}

fn has_source_extension(value: &str) -> bool {
    Path::new(value).extension().is_some_and(|ext| {
        ext.to_str()
            .map(str::to_ascii_lowercase)
            .is_some_and(|ext| matches!(ext.as_str(), "rs" | "md" | "toml"))
    })
}

fn keywords(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .filter_map(|word| {
            let lower = word.to_lowercase();
            (lower.len() >= 4).then_some(lower)
        })
        .collect()
}

fn keyword_score(text: &str, keywords: &[String]) -> usize {
    let lower = text.to_lowercase();
    keywords
        .iter()
        .filter(|keyword| lower.contains(keyword.as_str()))
        .count()
}

fn fallback_code_search(workspace: &Path, keywords: &[String]) -> Vec<ContextItem> {
    let files = git_output(workspace, &["ls-files"]).unwrap_or_default();
    files
        .lines()
        .filter(|file| has_source_extension(file))
        .filter_map(|file| {
            let path = workspace.join(file);
            let content = std::fs::read_to_string(&path).ok()?;
            (keyword_score(&content, keywords) > 0).then(|| {
                let excerpt = content.lines().take(40).collect::<Vec<_>>().join("\n");
                context_item(
                    file,
                    file,
                    &excerpt,
                    "file_search",
                    file,
                    Some(file.to_string()),
                    "fallback code search found query terms in this tracked file",
                    vec!["code".to_string(), "fallback".to_string()],
                )
            })
        })
        .take(MAX_CODE_ITEMS)
        .collect()
}

fn read_file_excerpt(path: &Path) -> Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read target {}", path.display()))?;
    Ok(content.lines().take(80).collect::<Vec<_>>().join("\n"))
}

fn summarize_directory(path: &Path) -> String {
    let Ok(entries) = std::fs::read_dir(path) else {
        return "Directory could not be read.".to_string();
    };
    entries
        .filter_map(Result::ok)
        .take(30)
        .map(|entry| entry.path())
        .filter_map(|path: PathBuf| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn estimate_packet_words(packet: &ContextPacket) -> usize {
    packet
        .sections
        .iter()
        .flat_map(|section| section.items.iter())
        .map(|item| item.token_estimate)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::autoresearch::{
        AutoresearchCommands, ProfileCommands, SourceCommands, handle_autoresearch,
    };
    use crate::context_packet_compiler::WorkspaceState;
    use crate::test_support::TestWorkspace;

    #[test]
    fn code_heavy_tasks_require_code_context() {
        assert!(is_code_heavy_task(
            "fix clippy failures in src/cmd/query.rs and add tests",
            &["src/cmd/query.rs".to_string()]
        ));
    }

    #[test]
    fn low_relevance_memory_only_cannot_be_high_confidence() {
        let quality = ResearchQuality::new(true, false, false, true);
        assert_eq!(quality.confidence, "low");
        assert!(!quality.passes_minimum_bar);
    }

    #[test]
    fn low_relevance_memory_with_code_is_not_high_confidence() {
        let quality = ResearchQuality::new(true, true, true, true);
        assert_eq!(quality.confidence, "medium");
        assert!(quality.passes_minimum_bar);
    }

    #[test]
    fn validation_commands_include_rust_release_gate_for_code_tasks() {
        let commands = infer_validation_commands(true, &["src/cmd/query.rs".to_string()]);
        assert!(
            commands
                .iter()
                .any(|cmd| cmd.command == "cargo test --workspace --all-targets")
        );
        assert!(
            commands
                .iter()
                .any(|cmd| cmd.command == "cargo clippy --workspace --all-targets -- -D warnings")
        );
        assert!(commands.iter().any(|cmd| cmd.command == "git diff --check"));
    }

    #[test]
    fn workspace_state_reports_dirty_warning() {
        let state = WorkspaceState {
            branch: Some("main".to_string()),
            head: Some("abc123".to_string()),
            dirty: true,
            changed_files: vec!["src/main.rs".to_string()],
            untracked_files: vec!["src/cmd/preflight.rs".to_string()],
            changed_total: 1,
            untracked_total: 1,
            truncated: false,
        };
        let warnings = state.warnings();
        assert!(
            warnings
                .iter()
                .any(|warning| warning.code == "dirty_worktree")
        );
    }

    #[test]
    fn preflight_packet_uses_preflight_route_and_id() {
        let args = PreflightArgs {
            task: "fix src/main.rs".to_string(),
            targets: Vec::new(),
            json: true,
            agent_prompt: false,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();

        assert_eq!(packet.route, "preflight");
        assert!(packet.id.starts_with("preflight-"));
        assert_eq!(packet.task, "fix src/main.rs");
    }

    #[test]
    fn preflight_rendering_does_not_call_it_autoresearch() {
        let args = PreflightArgs {
            task: "fix src/main.rs".to_string(),
            targets: Vec::new(),
            json: false,
            agent_prompt: false,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();
        let rendered = packet.to_markdown();

        assert!(rendered.contains("Route: preflight"));
        assert!(
            packet
                .sections
                .iter()
                .any(|section| section.title.contains("Preflight"))
        );
        assert!(!packet.route.contains("autoresearch"));
        assert!(!packet.why_retrieved.contains("Autoresearch"));
    }

    #[test]
    fn preflight_bridges_autoresearch_findings_into_context_packet() {
        let _ws = TestWorkspace::new("preflight-autoresearch-bridge");
        seed_autoresearch_store("preflight");
        let args = PreflightArgs {
            task: "fill context compiler autoresearch gap".to_string(),
            targets: vec!["src/cmd/autoresearch.rs".to_string()],
            json: true,
            agent_prompt: false,
            no_audit: true,
            strict: false,
        };

        let packet = build_preflight_packet(&args).unwrap();
        let section = packet
            .sections
            .iter()
            .find(|section| section.id == "autoresearch")
            .expect("preflight should include task-matched autoresearch findings");

        assert_eq!(packet.scores["autoresearch_findings"].as_u64(), Some(1));
        assert!(
            packet
                .evidence
                .contains("Context compiler autoresearch gap")
        );
        assert!(section.items[0].body.contains("Provenance:"));
        assert_eq!(section.items[0].source.kind, "autoresearch");
        assert!(
            packet
                .selection_trace
                .iter()
                .any(|trace| trace.item_id == "autoresearch-1")
        );
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

    #[test]
    fn strict_minimum_bar_requires_memory_section() {
        let mut packet = ContextPacket::new(
            "test".to_string(),
            "layers".to_string(),
            "fix src/main.rs".to_string(),
            chrono::Utc::now(),
        );
        packet.scores = json!({ "code_heavy": true });
        packet.sections.push(context_section(
            "workspace",
            "Workspace State",
            "Current Git/worktree context that can affect safe edits.",
            Vec::new(),
        ));
        packet.sections.push(code_impact_section(Vec::new()));
        packet.sections.push(context_section(
            "validation",
            "Validation",
            "Validation commands required after using this packet.",
            Vec::new(),
        ));

        assert!(!minimum_bar_passes(&packet));
    }
}
