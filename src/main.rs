use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod config;
mod council;
mod graph;
mod memory;
mod projects;
mod routing;
mod synthesis;
#[cfg(test)]
mod test_support;
mod types;
mod util;

use commands::{
    handle_council_promote, handle_council_run, handle_curated_import, handle_project_create,
    handle_project_list, handle_query, handle_refresh, handle_remember, handle_task_create,
    handle_task_list, handle_validate,
};

#[derive(Parser)]
#[command(name = "layers")]
#[command(about = "Local-first context router for Memoryport + GitNexus")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Query {
        query: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        no_audit: bool,
    },
    Refresh,
    Remember {
        kind: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        task_type: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        artifacts_dir: Option<String>,
        #[arg(long)]
        targets: Option<String>,
    },
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
    Validate,
    Curated {
        #[command(subcommand)]
        command: CuratedCommands,
    },
    Council {
        #[command(subcommand)]
        command: CouncilCommands,
    },
}

#[derive(Subcommand)]
enum ProjectCommands {
    Create {
        slug: String,
        title: String,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum TaskCommands {
    Create {
        project: String,
        slug: String,
        title: String,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        acceptance: Option<String>,
    },
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CuratedCommands {
    Import { file: String },
}

#[derive(Subcommand)]
enum CouncilCommands {
    Run {
        task: String,
        #[arg(long)]
        gemini_cmd: Option<String>,
        #[arg(long)]
        claude_cmd: Option<String>,
        #[arg(long)]
        codex_cmd: Option<String>,
        #[arg(long, default_value_t = 120)]
        timeout_secs: u64,
        #[arg(long, default_value_t = 1)]
        retry_limit: u32,
        #[arg(long)]
        artifacts_dir: Option<String>,
        #[arg(long)]
        targets: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Promote {
        run_id: String,
        #[arg(long)]
        project: String,
        #[arg(long)]
        artifacts_dir: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Query {
            query,
            json,
            no_audit,
        } => handle_query(&query, json, no_audit),
        Commands::Refresh => handle_refresh(),
        Commands::Remember {
            kind,
            task,
            task_type,
            summary,
            file,
            artifacts_dir,
            targets,
        } => handle_remember(
            &kind,
            task,
            task_type,
            summary,
            file,
            artifacts_dir,
            targets,
        ),
        Commands::Project { command } => match command {
            ProjectCommands::Create {
                slug,
                title,
                summary,
                status,
            } => handle_project_create(&slug, &title, summary, status),
            ProjectCommands::List { json } => handle_project_list(json),
        },
        Commands::Task { command } => match command {
            TaskCommands::Create {
                project,
                slug,
                title,
                summary,
                status,
                priority,
                acceptance,
            } => handle_task_create(
                &project, &slug, &title, summary, status, priority, acceptance,
            ),
            TaskCommands::List {
                project,
                status,
                json,
            } => handle_task_list(project, status, json),
        },
        Commands::Validate => handle_validate(),
        Commands::Curated { command } => match command {
            CuratedCommands::Import { file } => handle_curated_import(&file),
        },
        Commands::Council { command } => match command {
            CouncilCommands::Run {
                task,
                gemini_cmd,
                claude_cmd,
                codex_cmd,
                timeout_secs,
                retry_limit,
                artifacts_dir,
                targets,
                json,
            } => handle_council_run(
                &task,
                gemini_cmd,
                claude_cmd,
                codex_cmd,
                timeout_secs,
                retry_limit,
                artifacts_dir,
                targets,
                json,
            ),
            CouncilCommands::Promote {
                run_id,
                project,
                artifacts_dir,
                dry_run,
                json,
            } => handle_council_promote(&run_id, &project, artifacts_dir, dry_run, json),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::config::workspace_root;
    use crate::graph::normalize_graph_output;
    use crate::routing::route_query;
    use crate::synthesis::build_context;
    use crate::types::{
        BlastRadius, GitNexusIndexVersion, GraphContext, ImpactSummary, MemoryHit, RouteDecision,
    };
    use serde_json::json;

    #[test]
    fn route_memory_query_correctly() {
        let d = route_query(
            "What did we decide last time about Layers and how should it relate to GitNexus?",
        );
        assert_eq!(d.route, "memory_only");
    }

    #[test]
    fn route_graph_query_correctly() {
        let d = route_query(
            "What files and dependencies are involved in the council tool architecture?",
        );
        assert_eq!(d.route, "graph_only");
    }

    #[test]
    fn route_both_query_correctly() {
        let d = route_query("Implement the approved Layers design in the current repo layout.");
        assert_eq!(d.route, "both");
    }

    #[test]
    fn route_local_query_to_neither() {
        let d = route_query("Rename this variable in the snippet below.");
        assert_eq!(d.route, "neither");
    }

    #[test]
    fn normalize_graph_process_symbols() {
        let raw = r#"{
          "process_symbols": [
            {"name":"route_query","type":"function","filePath":"src/main.rs","step_index":1}
          ]
        }"#;
        let facts = normalize_graph_output(raw, 5);
        assert!(facts.iter().any(|f| f.contains("Process symbol: route_query [function] in src/main.rs step 1")));
    }

    #[test]
    fn find_git_root_from_repo() {
        let root = workspace_root();
        assert!(root.exists());
    }

    #[test]
    fn build_context_includes_typed_memory_brief_and_architecture_summary() {
        let decision = RouteDecision {
            route: "both".to_string(),
            confidence: "high".to_string(),
            scores: serde_json::Map::from_iter(vec![("historical".to_string(), json!(5))]),
            matches: serde_json::Map::new(),
            rationale: vec!["test".to_string()],
        };
        let memory_hits = vec![MemoryHit {
            kind: "decision".to_string(),
            score: Some(8.0),
            timestamp: Some("2026-03-31T00:00:00Z".to_string()),
            task: Some("layers".to_string()),
            summary: "Prefer curated memory over loose historical snippets.".to_string(),
            artifacts_dir: None,
            source: "project-records".to_string(),
            graph_context: Some(GraphContext {
                gitnexus_index_version: GitNexusIndexVersion {
                    indexed_at: "2026-03-31T00:00:00Z".to_string(),
                    last_commit: String::new(),
                    stats: json!({}),
                },
                impact_summary: Some(ImpactSummary {
                    target_symbols: vec!["handle_remember".to_string()],
                    blast_radius: BlastRadius {
                        direct: 1,
                        indirect: 0,
                        transitive: 1,
                    },
                    risk_level: "low".to_string(),
                    affected_processes: vec!["main".to_string()],
                }),
                implementation_context: None,
                review_context: None,
            }),
        }];
        let graph_hits =
            vec!["Definition: build_context [function] in src/synthesis.rs".to_string()];

        let payload = build_context(
            "test query",
            &decision,
            &memory_hits,
            None,
            &graph_hits,
            None,
        )
        .unwrap();

        assert!(
            payload
                .get("memory_brief")
                .and_then(|v| v.get("decisions"))
                .and_then(|v| v.as_array())
                .is_some()
        );
        assert!(
            payload
                .get("architecture_summary")
                .and_then(|v| v.as_array())
                .is_some_and(|items| !items.is_empty())
        );
        assert!(
            payload
                .get("structural_context")
                .and_then(|v| v.as_array())
                .is_some_and(|items| !items.is_empty())
        );
    }

    #[test]
    fn build_context_truncates_instead_of_failing_when_evidence_is_large() {
        let decision = RouteDecision {
            route: "both".to_string(),
            confidence: "high".to_string(),
            scores: serde_json::Map::new(),
            matches: serde_json::Map::new(),
            rationale: vec!["test".to_string()],
        };
        let long_summary =
            "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu ".repeat(30);
        let memory_hits = (0..3)
            .map(|index| MemoryHit {
                kind: if index == 0 {
                    "decision".to_string()
                } else {
                    "status".to_string()
                },
                score: Some(8.0),
                timestamp: Some("2026-03-31T00:00:00Z".to_string()),
                task: Some(format!("task-{}", index)),
                summary: long_summary.clone(),
                artifacts_dir: None,
                source: "project-records".to_string(),
                graph_context: None,
            })
            .collect::<Vec<_>>();
        let graph_hits = vec![
            long_summary.clone(),
            long_summary.clone(),
            long_summary.clone(),
        ];

        let payload = build_context(
            "very large query",
            &decision,
            &memory_hits,
            None,
            &graph_hits,
            None,
        )
        .unwrap();
        let context_text = payload
            .get("context_text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(context_text.split_whitespace().count() <= 1200);
    }
}
