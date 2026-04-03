use anyhow::Result;
use clap::{Parser, Subcommand};

mod cmd;
mod config;
mod council;
mod graph;
mod memory;
#[cfg(test)]
mod test_support;
mod types;
mod util;

mod router;

use cmd::council::{handle_council_promote, handle_council_run};
use cmd::curated::handle_curated_import;
use cmd::project::{handle_project_create, handle_project_list};
use cmd::query::handle_query;
use cmd::refresh::handle_refresh;
use cmd::remember::handle_remember;
use cmd::task::{handle_task_create, handle_task_list};
use cmd::validate::handle_validate;

/// Council orchestrator and memory spine for multi-model AI workflows.
#[derive(Parser)]
#[command(name = "layers", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Retrieve context for a task using heuristic routing.
    Query {
        /// The task or question to retrieve context for.
        task: String,
        /// Output structured JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
        /// Skip writing to the audit log.
        #[arg(long)]
        no_audit: bool,
    },
    /// Append a plan, learning, or trace record to the JSONL memory spine.
    Remember {
        /// Record kind: plan, learning, or trace.
        kind: String,
        /// Task description (required for plan and trace).
        #[arg(long)]
        task: Option<String>,
        /// Task type classification (e.g. architecture, bugfix).
        #[arg(long)]
        task_type: Option<String>,
        /// Human-readable summary of the record.
        #[arg(long)]
        summary: Option<String>,
        /// Path to a markdown file to attach (required for plan).
        #[arg(long)]
        file: Option<String>,
        /// Path to the artifacts directory for this record.
        #[arg(long)]
        artifacts_dir: Option<String>,
        /// Comma-separated symbol names for graph context.
        #[arg(long)]
        targets: Option<String>,
    },
    /// Run a self-test to verify council config and JSONL stores.
    Validate {
        /// Run routing benchmarks from an answer-key JSONL file.
        #[arg(long)]
        routing: Option<String>,
        /// Exit with non-zero status when validation fails.
        #[arg(long)]
        ci: bool,
    },
    /// Refresh GitNexus index and verify MemoryPort readiness.
    Refresh {
        /// Also regenerate embeddings (passes --embeddings to gitnexus analyze).
        #[arg(long)]
        embeddings: bool,
    },
    /// Import or manage curated memory records.
    Curated {
        #[command(subcommand)]
        command: CuratedCommands,
    },
    /// Orchestrate multi-model council workflows.
    Council {
        #[command(subcommand)]
        command: CouncilCommands,
    },
    /// Manage projects in the memory spine.
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
    /// Manage tasks associated with projects.
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
}

#[derive(Subcommand)]
enum CuratedCommands {
    /// Import curated records from a JSONL file into canonical memory.
    Import {
        /// Path to the JSONL file to import.
        file: String,
    },
}

#[derive(Subcommand)]
enum CouncilCommands {
    /// Run a Gemini → Claude → Codex council on a task.
    Run {
        /// The task or question to deliberate on.
        task: String,
        /// Command to invoke Gemini (overrides LAYERS_COUNCIL_GEMINI_CMD).
        #[arg(long)]
        gemini_cmd: Option<String>,
        /// Command to invoke Claude (overrides LAYERS_COUNCIL_CLAUDE_CMD).
        #[arg(long)]
        claude_cmd: Option<String>,
        /// Command to invoke Codex (overrides LAYERS_COUNCIL_CODEX_CMD).
        #[arg(long)]
        codex_cmd: Option<String>,
        /// Per-stage timeout in seconds.
        #[arg(long, default_value_t = 120)]
        timeout_secs: u64,
        /// Max retry attempts per stage.
        #[arg(long, default_value_t = 1)]
        retry_limit: u32,
        /// Custom artifacts directory (default: memoryport/council-runs/<run-id>).
        #[arg(long)]
        artifacts_dir: Option<String>,
        /// Comma-separated symbol names for GitNexus impact context.
        #[arg(long)]
        targets: Option<String>,
        /// Output full JSON instead of human summary.
        #[arg(long)]
        json: bool,
    },
    /// Promote a converged council run into canonical curated memory.
    Promote {
        /// The run ID to promote (from council run output).
        run_id: String,
        /// Target project slug for the promoted decision.
        #[arg(long)]
        project: String,
        /// Custom artifacts directory to find the run in.
        #[arg(long)]
        artifacts_dir: Option<String>,
        /// Preview what would be promoted without writing.
        #[arg(long)]
        dry_run: bool,
        /// Output full JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ProjectCommands {
    /// Create a new project entry.
    Create {
        /// Project slug (short identifier).
        slug: String,
        /// Project title.
        #[arg(long)]
        title: String,
        /// Project summary.
        #[arg(long)]
        summary: Option<String>,
        /// Project status (default: active).
        #[arg(long)]
        status: Option<String>,
        /// Output JSON.
        #[arg(long)]
        json: bool,
    },
    /// List all non-archived projects.
    List {
        /// Output JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum TaskCommands {
    /// Create a new task within a project.
    Create {
        /// Project slug the task belongs to.
        project: String,
        /// Task slug (short identifier).
        slug: String,
        /// Task title.
        #[arg(long)]
        title: String,
        /// Task summary.
        #[arg(long)]
        summary: Option<String>,
        /// Task status (default: open).
        #[arg(long)]
        status: Option<String>,
        /// Output JSON.
        #[arg(long)]
        json: bool,
    },
    /// List tasks, optionally filtered by project or status.
    List {
        /// Filter by project slug.
        #[arg(long)]
        project: Option<String>,
        /// Filter by status.
        #[arg(long)]
        status: Option<String>,
        /// Output JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Query {
            task,
            json,
            no_audit,
        } => handle_query(&task, json, no_audit),
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
        Commands::Validate { routing, ci } => handle_validate(routing, ci),
        Commands::Refresh { embeddings } => handle_refresh(embeddings),
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
        Commands::Project { command } => match command {
            ProjectCommands::Create {
                slug,
                title,
                summary,
                status,
                json,
            } => handle_project_create(&slug, &title, summary, status, json),
            ProjectCommands::List { json } => handle_project_list(json),
        },
        Commands::Task { command } => match command {
            TaskCommands::Create {
                project,
                slug,
                title,
                summary,
                status,
                json,
            } => handle_task_create(&project, &slug, &title, summary, status, json),
            TaskCommands::List {
                project,
                status,
                json,
            } => handle_task_list(project.as_deref(), status.as_deref(), json),
        },
    }
}
