use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod config;
mod council;
#[cfg(test)]
mod test_support;
mod types;
mod util;

mod router;

use commands::{
    handle_council_promote, handle_council_run, handle_curated_import, handle_query,
    handle_remember, handle_validate,
};

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
    Validate,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Query { task, json } => handle_query(&task, json),
        Commands::Remember {
            kind,
            task,
            task_type,
            summary,
            file,
            artifacts_dir,
            targets,
        } => handle_remember(&kind, task, task_type, summary, file, artifacts_dir, targets),
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
