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
    clippy::result_large_err
)]

//! Layers — local-first `ContextPacket` compiler for coding agents.

#[cfg(feature = "deprecated-runtime")]
use std::path::PathBuf;

use clap::{Parser, Subcommand};
#[cfg(feature = "deprecated-runtime")]
use layers_daemon::lifecycle::DaemonRunner;
#[cfg(feature = "deprecated-runtime")]
use layers_store::config::ConfigStore;
use tracing_subscriber::EnvFilter;

mod cmd;
mod config;
mod context_packet_compiler;
mod council;
#[allow(dead_code)]
mod critical_path;
mod graph;
mod memory;
#[cfg(test)]
mod test_support;
mod types;
mod util;

mod feedback;
mod plugins;
mod quality;
mod router;
#[cfg(feature = "substrate-storage")]
mod technician;
mod uc;

pub mod memory_index;

use cmd::autoresearch::{AutoresearchCommands, handle_autoresearch};
use cmd::chat::{ChatArgs, handle_chat};
use cmd::config_cmd::{ConfigArgs, handle_config};
use cmd::council::{
    handle_council_list, handle_council_promote, handle_council_resume, handle_council_resume_last,
    handle_council_run, handle_council_status,
};
use cmd::curated::{
    handle_curated_audit, handle_curated_import, handle_curated_list, handle_curated_search,
    handle_curated_show,
};
use cmd::feedback::handle_feedback;
use cmd::gate::handle_gate;
use cmd::infrastructure::{InfrastructureArgs, handle_infrastructure};
use cmd::init::{InitArgs, handle_init};
use cmd::migrate::handle_migrate;
#[cfg(feature = "substrate-storage")]
use cmd::monitor::handle_monitor;
use cmd::packet::{PacketCommands, handle_packet};
use cmd::preflight::{PreflightArgs, handle_preflight};
use cmd::query::handle_query;
use cmd::refresh::handle_refresh;
use cmd::remember::handle_remember;
#[cfg(feature = "substrate-storage")]
use cmd::technician::handle_technician;
use cmd::telemetry::{TelemetryCommands, handle_telemetry};
use cmd::validate::handle_validate;
use cmd::workflow_benchmark::{WorkflowBenchmarkCommands, handle_workflow_benchmark};

/// Local-first `ContextPacket` compiler for coding agents.
#[derive(Parser)]
#[command(
    name = "layers",
    version,
    about = "Layers — local-first ContextPacket compiler for coding agents."
)]
struct Cli {
    /// Enable verbose tracing output (sets `RUST_LOG=layers=debug`).
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[cfg(feature = "deprecated-runtime")]
    /// [deprecated/experimental] Run the non-core daemon runtime.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
    /// [support] Bootstrap a new Layers workspace.
    Init {
        /// Force overwrite existing files.
        #[arg(long)]
        force: bool,
        /// Path to initialize (defaults to current workspace).
        #[arg(long)]
        path: Option<std::path::PathBuf>,
    },
    /// [deprecated/experimental] Start the non-core interactive chat loop.
    Chat {
        /// System prompt override.
        #[arg(long)]
        system_prompt: Option<String>,
        /// Model override (e.g. "openai/gpt-4").
        #[arg(long)]
        model: Option<String>,
        /// Maximum turns before exiting (0 = unlimited).
        #[arg(long, default_value_t = 0)]
        max_turns: usize,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// [stable core] Display and manage configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// [stable core] Retrieve context for a task.
    Query {
        /// The task or question to retrieve context for.
        task: String,
        /// Output structured JSON instead of human-readable text.
        #[arg(long, conflicts_with = "agent_prompt")]
        json: bool,
        /// Output an agent-ready prompt block.
        #[arg(long, conflicts_with = "json")]
        agent_prompt: bool,
        /// Skip writing to the audit log.
        #[arg(long)]
        no_audit: bool,
        /// Minimum number of UC semantic results before surfacing a warning.
        /// If UC returns fewer than this, a warning appears in the output.
        #[arg(long, default_value = "3")]
        uc_min_results: usize,
    },
    /// [stable core] Validate, inspect, render, and diff `ContextPacket` artifacts.
    Packet {
        #[command(subcommand)]
        command: PacketCommands,
    },
    /// [beta] Prepare a local pre-edit context packet for a task.
    Preflight {
        /// The task or question to prepare context for before editing.
        task: String,
        /// File, directory, symbol, or test target to prioritize. Repeatable.
        #[arg(long = "target")]
        targets: Vec<String>,
        /// Output structured JSON.
        #[arg(long, conflicts_with = "agent_prompt")]
        json: bool,
        /// Output an agent-ready prompt block.
        #[arg(long, conflicts_with = "json")]
        agent_prompt: bool,
        /// Skip audit side effects.
        #[arg(long)]
        no_audit: bool,
        /// Fail if minimum code-heavy context coverage is not met.
        #[arg(long)]
        strict: bool,
    },
    /// [beta] Track, scan, and search external research sources.
    Autoresearch {
        /// Autoresearch subcommand.
        #[command(subcommand)]
        command: AutoresearchCommands,
    },
    /// [stable core] Append explicit memory to the JSONL spine.
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
    /// [stable core] Verify local readiness and degraded modes.
    Validate {
        /// Run routing benchmarks from an answer-key JSONL file.
        #[arg(long)]
        routing: Option<String>,
        /// Exit with non-zero status when validation fails.
        #[arg(long)]
        ci: bool,
    },
    /// [stable core] Refresh GitNexus/MemoryPort derived context.
    Refresh {
        /// Also regenerate embeddings (passes --embeddings to gitnexus analyze).
        #[arg(long)]
        embeddings: bool,
    },
    /// [support] Run format, compile, clippy, test, audit, and MCP checks.
    Gate {
        /// Skip the MCP connectivity check (useful when gitnexus-rs is not on PATH).
        #[arg(long)]
        skip_mcp: bool,
        /// Override the timeout for `cargo audit` in seconds.
        #[arg(long, default_value = "120")]
        audit_timeout: u64,
        /// Path to the workspace to gate. Defaults to the current directory.
        #[arg(long)]
        workspace: Option<std::path::PathBuf>,
    },
    /// [support] Record a route correction for context selection.
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
    /// [stable core] Import or manage curated memory records.
    Curated {
        #[command(subcommand)]
        command: CuratedCommands,
    },
    /// [beta] Run/promote council workflows as memory producers.
    Council {
        #[command(subcommand)]
        command: CouncilCommands,
    },
    /// [deprecated/experimental] Manage infrastructure credentials.
    Infrastructure {
        #[command(subcommand)]
        command: InfrastructureCommands,
    },
    /// [support] Migrate legacy project records into curated memory.
    Migrate {
        /// Preview what would be migrated without writing.
        #[arg(long)]
        dry_run: bool,
    },
    #[cfg(feature = "substrate-storage")]
    /// [deprecated/experimental] Run the non-core autonomous repo monitor.
    Monitor {
        #[command(subcommand)]
        command: cmd::monitor::MonitorArgs,
    },
    #[cfg(feature = "substrate-storage")]
    /// [deprecated/experimental] Run non-core self-healing integration checks.
    Technician {
        #[command(subcommand)]
        command: cmd::technician::TechnicianArgs,
    },
    /// [deprecated/experimental] View integration telemetry and health reports.
    Telemetry {
        #[command(subcommand)]
        command: TelemetryCommands,
    },
    /// [stable core] Analyze Layers-vs-baseline workflow benchmark telemetry.
    WorkflowBenchmark {
        #[command(subcommand)]
        command: WorkflowBenchmarkCommands,
    },
}

#[derive(Subcommand)]
enum CuratedCommands {
    /// Import curated records from a JSONL file into canonical memory.
    Import {
        /// Path to the JSONL file to import.
        file: String,
    },
    /// List canonical curated memory records.
    List {
        /// Maximum number of records to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Include legacy council JSONL adapter records.
        #[arg(long)]
        include_legacy: bool,
    },
    /// Search canonical curated memory records.
    Search {
        /// Search query.
        query: String,
        /// Maximum number of records to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Include legacy council JSONL adapter records.
        #[arg(long)]
        include_legacy: bool,
    },
    /// Show one canonical curated memory record.
    Show {
        /// Record id to show.
        id: String,
        /// Include legacy council JSONL adapter records.
        #[arg(long)]
        include_legacy: bool,
    },
    /// Audit canonical memory and legacy adapter counts.
    Audit,
}

#[cfg(feature = "deprecated-runtime")]
#[derive(Subcommand)]
enum DaemonCommands {
    /// Start the Layers daemon.
    Run {
        /// Path to `layers.toml`. Defaults to `<workspace>/layers.toml`.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Optional PID file path to write while the daemon is running.
        #[arg(long)]
        pid_file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum InfrastructureCommands {
    /// Interactive setup wizard for infrastructure credentials.
    Setup,
    /// List all configured providers.
    List,
    /// Remove credentials for a provider.
    Remove { provider: String },
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
    Remove { alias: String },
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
enum ConfigCommands {
    /// Display the resolved configuration (secrets masked).
    Show,
    /// Display config file paths.
    Path,
    /// Validate configuration.
    Validate,
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
        /// Mark this run as critical-path (latency-sensitive, gets priority
        /// scheduling via the weighted fair queue and reserved worker slot).
        /// Without this flag, the route heuristic decides automatically.
        #[arg(long)]
        critical_path: bool,
    },
    /// Resume a previously interrupted council run.
    Resume {
        /// The run ID to resume.
        run_id: String,
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
        json: bool,
    },
    /// Resume the most recent incomplete council run.
    ResumeLast {
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
        json: bool,
    },
    /// Show council run status plus checkpoint state.
    Status {
        run_id: String,
        #[arg(long)]
        artifacts_dir: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List recent council runs.
    List {
        #[arg(long, default_value_t = 10)]
        limit: usize,
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
    init_tracing(cli.verbose);
    match cli.command {
        Commands::Init { force, path } => handle_init(&InitArgs { force, path }),
        Commands::Chat {
            system_prompt,
            model,
            max_turns,
            json,
        } => handle_chat(&ChatArgs {
            system_prompt,
            model,
            max_turns,
            json,
        }),
        Commands::Config { command } => handle_config(&match command {
            ConfigCommands::Show => ConfigArgs::Show,
            ConfigCommands::Path => ConfigArgs::Path,
            ConfigCommands::Validate => ConfigArgs::Validate,
        }),
        #[cfg(feature = "deprecated-runtime")]
        Commands::Daemon { command } => match command {
            DaemonCommands::Run { config, pid_file } => handle_daemon_run(config, pid_file),
        },
        Commands::Query {
            task,
            json,
            agent_prompt,
            no_audit,
            uc_min_results,
        } => handle_query(&task, json, agent_prompt, no_audit, uc_min_results),
        Commands::Packet { command } => handle_packet(&command),
        Commands::Preflight {
            task,
            targets,
            json,
            agent_prompt,
            no_audit,
            strict,
        } => handle_preflight(&PreflightArgs {
            task,
            targets,
            json,
            agent_prompt,
            no_audit,
            strict,
        }),
        Commands::Autoresearch { command } => handle_autoresearch(&command),
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
        Commands::Gate {
            skip_mcp,
            audit_timeout,
            workspace,
        } => handle_gate(&cmd::gate::GateArgs {
            skip_mcp,
            audit_timeout,
            workspace,
        }),
        Commands::Feedback {
            task,
            predicted,
            actual,
        } => {
            let args = cmd::feedback::FeedbackArgs {
                task,
                predicted: predicted
                    .parse()
                    .map_err(|e: String| anyhow::anyhow!("{e}"))?,
                actual: actual.parse().map_err(|e: String| anyhow::anyhow!("{e}"))?,
            };
            handle_feedback(&args)
        }
        Commands::Curated { command } => match command {
            CuratedCommands::Import { file } => handle_curated_import(&file),
            CuratedCommands::List {
                limit,
                include_legacy,
            } => handle_curated_list(limit, include_legacy),
            CuratedCommands::Search {
                query,
                limit,
                include_legacy,
            } => handle_curated_search(&query, limit, include_legacy),
            CuratedCommands::Show { id, include_legacy } => {
                handle_curated_show(&id, include_legacy)
            }
            CuratedCommands::Audit => handle_curated_audit(),
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
                critical_path,
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
                critical_path,
            ),
            CouncilCommands::Resume {
                run_id,
                gemini_cmd,
                claude_cmd,
                codex_cmd,
                timeout_secs,
                retry_limit,
                artifacts_dir,
                json,
            } => handle_council_resume(
                &run_id,
                gemini_cmd,
                claude_cmd,
                codex_cmd,
                timeout_secs,
                retry_limit,
                artifacts_dir,
                json,
            ),
            CouncilCommands::ResumeLast {
                gemini_cmd,
                claude_cmd,
                codex_cmd,
                timeout_secs,
                retry_limit,
                json,
            } => handle_council_resume_last(
                gemini_cmd,
                claude_cmd,
                codex_cmd,
                timeout_secs,
                retry_limit,
                json,
            ),
            CouncilCommands::Status {
                run_id,
                artifacts_dir,
                json,
            } => handle_council_status(&run_id, artifacts_dir, json),
            CouncilCommands::List { limit, json } => handle_council_list(limit, json),
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
                InfrastructureCommands::Remove { provider } => {
                    InfrastructureArgs::Remove { provider }
                }
                InfrastructureCommands::Test => InfrastructureArgs::Test,
                InfrastructureCommands::Ssh { command } => InfrastructureArgs::Ssh {
                    command: match command {
                        SshCommands::Add {
                            alias,
                            connection,
                            key,
                            provider,
                        } => cmd::infrastructure::SshCommands::Add {
                            alias,
                            connection,
                            key,
                            provider,
                        },
                        SshCommands::List => cmd::infrastructure::SshCommands::List,
                        SshCommands::Remove { alias } => {
                            cmd::infrastructure::SshCommands::Remove { alias }
                        }
                    },
                },
                InfrastructureCommands::Webhook { command } => InfrastructureArgs::Webhook {
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
                        WebhookCommands::Status => cmd::infrastructure::WebhookCommands::Status,
                        WebhookCommands::Remove => cmd::infrastructure::WebhookCommands::Remove,
                    },
                },
            };
            handle_infrastructure(&args)
        }
        Commands::Migrate { dry_run } => handle_migrate(dry_run),
        #[cfg(feature = "substrate-storage")]
        Commands::Monitor { command } => handle_monitor(&command),
        #[cfg(feature = "substrate-storage")]
        Commands::Technician { command } => handle_technician(&command),
        Commands::Telemetry { command } => handle_telemetry(&command),
        Commands::WorkflowBenchmark { command } => handle_workflow_benchmark(&command),
    }
}

/// Initialise the `tracing` subscriber.
///
/// * If `verbose` is true → `RUST_LOG=layers=debug` (unless the caller already
///   set `RUST_LOG`).
/// * Otherwise → respect `RUST_LOG` or default to `layers=warn`.
///
/// The subscriber is installed exactly once; subsequent calls are no-ops (the
/// standard `tracing` guard pattern for tests).
fn init_tracing(verbose: bool) {
    let default = if verbose {
        "layers=debug"
    } else {
        "layers=warn"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

#[cfg(feature = "deprecated-runtime")]
fn handle_daemon_run(config: Option<PathBuf>, pid_file: Option<PathBuf>) -> anyhow::Result<()> {
    let config = crate::config::load_config_with_precedence(config.as_deref())?;
    ConfigStore::validate(&config)?;

    let db_path = crate::config::workspace_root().join("layers.db");
    let config_providers: Vec<_> = config.providers.clone().into_iter().collect();

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let (runner, _inbound_rx) = DaemonRunner::new(config.daemon.clone());
        let runner = if let Some(pid_file) = pid_file {
            runner.with_pid_file(pid_file)
        } else {
            runner
        };
        let runner = if config.brains.is_empty() {
            runner
        } else {
            let workdir = config
                .agent
                .workspace
                .clone()
                .unwrap_or_else(|| ".".to_string());
            runner.with_brains(config.brains.clone(), workdir)
        };

        runner
            .bootstrap_providers(&db_path, &config_providers)
            .await?;
        runner.run().await.map_err(anyhow::Error::from)
    })
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands};
    use crate::cmd::packet::{PacketCommands, PacketRenderFormat};
    use crate::cmd::workflow_benchmark::WorkflowBenchmarkCommands;
    use clap::{CommandFactory, Parser};
    use std::path::PathBuf;

    #[test]
    fn cli_about_positions_layers_as_context_packet_compiler() {
        let command = Cli::command();
        let about = command
            .get_about()
            .expect("CLI should expose concise product positioning")
            .to_string();

        assert!(about.contains("local-first ContextPacket compiler"));
        assert!(!about.contains("council orchestrator"));
        assert!(!about.contains("memory spine"));
    }

    #[test]
    fn cli_reference_documents_packet_artifact_commands() {
        let docs = include_str!("../docs/cli.md");

        assert!(docs.contains("## `layers packet validate <packet.json>`"));
        assert!(docs.contains("## `layers packet inspect <packet.json>`"));
        assert!(docs.contains("## `layers packet render <packet.json>`"));
        assert!(docs.contains("## `layers packet diff <old.json> <new.json>`"));
        assert!(docs.contains("objective-brief"));
        assert!(docs.contains("artifact-only"));
    }

    #[test]
    fn north_star_names_packet_artifacts_as_stable_core() {
        let docs = include_str!("../docs/NORTH_STAR.md");

        assert!(docs.contains("`layers packet validate`"));
        assert!(docs.contains("`layers packet inspect`"));
        assert!(docs.contains("`layers packet render`"));
        assert!(docs.contains("`layers packet diff`"));
        assert!(!docs.contains("context/memory spine"));
    }

    #[test]
    fn parses_packet_validate_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "packet",
            "validate",
            "docs/examples/context-packet-v2-minimal.json",
            "--strict",
            "--json",
        ])
        .expect("packet validate should parse");

        let Commands::Packet { command } = cli.command else {
            panic!("expected packet command");
        };
        let PacketCommands::Validate { path, strict, json } = command else {
            panic!("expected packet validate command");
        };
        assert_eq!(
            path,
            PathBuf::from("docs/examples/context-packet-v2-minimal.json")
        );
        assert!(strict);
        assert!(json);
    }

    #[test]
    fn parses_packet_render_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "packet",
            "render",
            "packet.json",
            "--format",
            "agent-prompt",
        ])
        .expect("packet render should parse");

        let Commands::Packet { command } = cli.command else {
            panic!("expected packet command");
        };
        let PacketCommands::Render { path, format } = command else {
            panic!("expected packet render command");
        };
        assert_eq!(path, PathBuf::from("packet.json"));
        assert_eq!(format, PacketRenderFormat::AgentPrompt);
    }

    #[test]
    fn parses_packet_render_objective_brief_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "packet",
            "render",
            "packet.json",
            "--format",
            "objective-brief",
        ])
        .expect("packet objective brief render should parse");

        let Commands::Packet { command } = cli.command else {
            panic!("expected packet command");
        };
        let PacketCommands::Render { path, format } = command else {
            panic!("expected packet render command");
        };
        assert_eq!(path, PathBuf::from("packet.json"));
        assert_eq!(format, PacketRenderFormat::ObjectiveBrief);
    }

    #[test]
    fn parses_packet_inspect_command() {
        let cli = Cli::try_parse_from(["layers", "packet", "inspect", "packet.json", "--json"])
            .expect("packet inspect should parse");

        let Commands::Packet { command } = cli.command else {
            panic!("expected packet command");
        };
        let PacketCommands::Inspect { path, json } = command else {
            panic!("expected packet inspect command");
        };
        assert_eq!(path, PathBuf::from("packet.json"));
        assert!(json);
    }

    #[test]
    fn parses_packet_diff_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "packet",
            "diff",
            "old-packet.json",
            "new-packet.json",
            "--json",
        ])
        .expect("packet diff should parse");

        let Commands::Packet { command } = cli.command else {
            panic!("expected packet command");
        };
        let PacketCommands::Diff { old, new, json } = command else {
            panic!("expected packet diff command");
        };
        assert_eq!(old, PathBuf::from("old-packet.json"));
        assert_eq!(new, PathBuf::from("new-packet.json"));
        assert!(json);
    }

    #[test]
    fn parses_workflow_benchmark_analyze_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "workflow-benchmark",
            "analyze",
            "runs.jsonl",
            "--json",
        ])
        .expect("workflow benchmark analyze should parse");

        let Commands::WorkflowBenchmark { command } = cli.command else {
            panic!("expected workflow benchmark command");
        };
        let WorkflowBenchmarkCommands::Analyze { path, json } = command else {
            panic!("expected workflow benchmark analyze command");
        };
        assert_eq!(path, PathBuf::from("runs.jsonl"));
        assert!(json);
    }

    #[test]
    fn parses_workflow_benchmark_validate_tasks_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "workflow-benchmark",
            "validate-tasks",
            "benchmarks/workflows/tasks",
            "--json",
        ])
        .expect("workflow benchmark validate-tasks should parse");

        let Commands::WorkflowBenchmark { command } = cli.command else {
            panic!("expected workflow benchmark command");
        };
        let WorkflowBenchmarkCommands::ValidateTasks { path, json } = command else {
            panic!("expected workflow benchmark validate-tasks command");
        };
        assert_eq!(path, PathBuf::from("benchmarks/workflows/tasks"));
        assert!(json);
    }

    #[test]
    fn parses_workflow_benchmark_plan_run_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "workflow-benchmark",
            "plan-run",
            "benchmarks/workflows/tasks",
            "--output-dir",
            "docs/dogfood/run",
            "--repo-root",
            "/repo/layers",
            "--agent-command",
            "codex exec",
            "--model",
            "test-model",
            "--seed",
            "42",
            "--json",
        ])
        .expect("workflow benchmark plan-run should parse");

        let Commands::WorkflowBenchmark { command } = cli.command else {
            panic!("expected workflow benchmark command");
        };
        let WorkflowBenchmarkCommands::PlanRun {
            path,
            output_dir,
            repo_root,
            agent_command,
            model,
            seed,
            json,
        } = command
        else {
            panic!("expected workflow benchmark plan-run command");
        };
        assert_eq!(path, PathBuf::from("benchmarks/workflows/tasks"));
        assert_eq!(output_dir, PathBuf::from("docs/dogfood/run"));
        assert_eq!(repo_root, PathBuf::from("/repo/layers"));
        assert_eq!(agent_command, "codex exec");
        assert_eq!(model.as_deref(), Some("test-model"));
        assert_eq!(seed, 42);
        assert!(json);
    }

    #[test]
    fn parses_workflow_benchmark_run_plan_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "workflow-benchmark",
            "run-plan",
            "docs/dogfood/run/runner-plan.json",
            "--preflight-command",
            "layers preflight --no-audit --json --strict",
            "--keep-worktrees",
            "--json",
        ])
        .expect("workflow benchmark run-plan should parse");

        let Commands::WorkflowBenchmark { command } = cli.command else {
            panic!("expected workflow benchmark command");
        };
        let WorkflowBenchmarkCommands::RunPlan {
            path,
            preflight_command,
            keep_worktrees,
            json,
        } = command
        else {
            panic!("expected workflow benchmark run-plan command");
        };
        assert_eq!(path, PathBuf::from("docs/dogfood/run/runner-plan.json"));
        assert_eq!(
            preflight_command,
            "layers preflight --no-audit --json --strict"
        );
        assert!(keep_worktrees);
        assert!(json);
    }

    #[test]
    fn parses_workflow_benchmark_retrieval_eval_corpus_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "workflow-benchmark",
            "retrieval-eval-corpus",
            "benchmarks/workflows/tasks",
            "--repo-root",
            "/repo/layers",
            "--json",
        ])
        .expect("workflow benchmark retrieval-eval-corpus should parse");

        let Commands::WorkflowBenchmark { command } = cli.command else {
            panic!("expected workflow benchmark command");
        };
        let WorkflowBenchmarkCommands::RetrievalEvalCorpus {
            path,
            repo_root,
            json,
        } = command
        else {
            panic!("expected workflow benchmark retrieval-eval-corpus command");
        };
        assert_eq!(path, PathBuf::from("benchmarks/workflows/tasks"));
        assert_eq!(repo_root, PathBuf::from("/repo/layers"));
        assert!(json);
    }

    #[test]
    fn parses_workflow_benchmark_retrieval_eval_lexical_command() {
        let cli = Cli::try_parse_from([
            "layers",
            "workflow-benchmark",
            "retrieval-eval-lexical",
            "target/retrieval-proof/retrieval-eval-corpus.json",
            "--json",
        ])
        .expect("workflow benchmark retrieval-eval-lexical should parse");

        let Commands::WorkflowBenchmark { command } = cli.command else {
            panic!("expected workflow benchmark command");
        };
        let WorkflowBenchmarkCommands::RetrievalEvalLexical { path, json } = command else {
            panic!("expected workflow benchmark retrieval-eval-lexical command");
        };
        assert_eq!(
            path,
            PathBuf::from("target/retrieval-proof/retrieval-eval-corpus.json")
        );
        assert!(json);
    }
}
