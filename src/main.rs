#![deny(warnings)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_variables)]
#![deny(unused_must_use)]
#![deny(unreachable_pub)]
#![deny(elided_lifetimes_in_paths)]
#![warn(missing_docs)]

// Binary crate — all items are pub for internal clarity but not exported as a library.
#![allow(unreachable_pub)]

// Structural lints that cannot be fixed without invasive refactoring:
#![allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::unnecessary_wraps,
    clippy::result_large_err,
)]

//! Layers — council orchestrator and memory spine for multi-model AI workflows.


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
mod uc;

use cmd::council::{handle_council_promote, handle_council_run};
use cmd::curated::handle_curated_import;
use cmd::feedback::handle_feedback;
use cmd::infrastructure::{handle_infrastructure, InfrastructureArgs};
use cmd::monitor::handle_monitor;
use cmd::query::handle_query;
use cmd::refresh::handle_refresh;
use cmd::remember::handle_remember;
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
    /// Refresh `GitNexus` index and verify `MemoryPort` readiness.
    Refresh {
        /// Also regenerate embeddings (passes --embeddings to gitnexus analyze).
        #[arg(long)]
        embeddings: bool,
    },
    /// Record a route correction to improve future routing decisions.
    Feedback {
        /// The task text that was originally classified.
        task: String,
        /// The route the system originally predicted.
        #[arg(long)]
        predicted: String,
        /// The route that was actually correct.
        #[arg(long)]
        actual: String,
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
    /// Manage cloud and infrastructure credentials (SSH, Fly.io, Vercel, Cloudflare, Hetzner, Render, Railway, GitHub, Webhook relay).
    Infrastructure {
        #[command(subcommand)]
        command: InfrastructureCommands,
    },
    /// Autonomous repo monitor: git sync, build/test checks, CI watching, fix subagents.
    Monitor {
        #[command(subcommand)]
        command: cmd::monitor::MonitorArgs,
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
enum InfrastructureCommands {
    /// Interactive setup wizard for infrastructure credentials.
    Setup,
    /// List all configured providers.
    List,
    /// Remove credentials for a provider.
    Remove {
        provider: String,
    },
    /// Test connectivity to all configured providers.
    Test,
    /// Manage SSH host aliases.
    Ssh {
        #[command(subcommand)]
        command: SshCommands,
    },
    /// Manage GitHub webhook relay endpoint.
    Webhook {
        #[command(subcommand)]
        command: WebhookCommands,
    },
}

#[derive(Subcommand)]
enum SshCommands {
    /// Add an SSH host alias.
    Add {
        alias: String,
        connection: String,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        provider: Option<String>,
    },
    /// List all SSH host aliases.
    List,
    /// Remove an SSH host alias.
    Remove {
        alias: String,
    },
}

#[derive(Subcommand)]
enum WebhookCommands {
    /// Configure a GitHub webhook relay via Cloudflare Worker.
    Setup {
        #[arg(long)]
        cf_token: Option<String>,
        #[arg(long)]
        cf_account: Option<String>,
        #[arg(long)]
        github_secret: Option<String>,
    },
    /// Show current webhook relay URL and status.
    Status,
    /// Remove webhook relay configuration.
    Remove,
}

#[derive(Subcommand)]
enum CouncilCommands {
    /// Run a Gemini → Claude → Codex council on a task.
    Run {
        /// The task or question to deliberate on.
        task: String,
        /// Command to invoke Gemini (overrides `LAYERS_COUNCIL_GEMINI_CMD`).
        #[arg(long)]
        gemini_cmd: Option<String>,
        /// Command to invoke Claude (overrides `LAYERS_COUNCIL_CLAUDE_CMD`).
        #[arg(long)]
        claude_cmd: Option<String>,
        /// Command to invoke Codex (overrides `LAYERS_COUNCIL_CODEX_CMD`).
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
        /// Comma-separated symbol names for `GitNexus` impact context.
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

fn main() -> anyhow::Result<()> {
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
        Commands::Feedback { task, predicted, actual } => {
            let args = cmd::feedback::FeedbackArgs {
                task,
                predicted: predicted.parse().map_err(|e: String| anyhow::anyhow!("{e}"))?,
                actual: actual.parse().map_err(|e: String| anyhow::anyhow!("{e}"))?,
            };
            handle_feedback(&args)
        }
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
        Commands::Infrastructure { command } => {
            let args = match command {
                InfrastructureCommands::Setup => InfrastructureArgs::Setup,
                InfrastructureCommands::List => InfrastructureArgs::List,
                InfrastructureCommands::Remove { provider } => InfrastructureArgs::Remove { provider },
                InfrastructureCommands::Test => InfrastructureArgs::Test,
                InfrastructureCommands::Ssh { command } => {
                    InfrastructureArgs::Ssh {
                        command: match command {
                            SshCommands::Add { alias, connection, key, provider } => {
                                cmd::infrastructure::SshCommands::Add {
                                    alias,
                                    connection,
                                    key,
                                    provider,
                                }
                            }
                            SshCommands::List => cmd::infrastructure::SshCommands::List,
                            SshCommands::Remove { alias } => {
                                cmd::infrastructure::SshCommands::Remove { alias }
                            }
                        },
                    }
                }
                InfrastructureCommands::Webhook { command } => {
                    InfrastructureArgs::Webhook {
                        command: match command {
                            WebhookCommands::Setup {
                                cf_token,
                                cf_account,
                                github_secret,
                            } => cmd::infrastructure::WebhookCommands::Setup {
                                cf_token,
                                cf_account,
                                github_secret,
                            },
                            WebhookCommands::Status => {
                                cmd::infrastructure::WebhookCommands::Status
                            }
                            WebhookCommands::Remove => {
                                cmd::infrastructure::WebhookCommands::Remove
                            }
                        },
                    }
                }
            };
            handle_infrastructure(&args)
        }
        Commands::Monitor { command } => handle_monitor(&command),
    }
}
