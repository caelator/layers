use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum PacketRenderFormat {
    Markdown,
    AgentPrompt,
    Json,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PacketCommands {
    /// Validate a `ContextPacket` artifact from disk.
    Validate {
        /// Path to a `ContextPacket` JSON artifact.
        path: PathBuf,
        /// Fail on packet quality issues that are warnings by default.
        #[arg(long)]
        strict: bool,
        /// Output a structured JSON validation report.
        #[arg(long)]
        json: bool,
    },
    /// Inspect a `ContextPacket` artifact without recompiling it.
    Inspect {
        /// Path to a `ContextPacket` JSON artifact.
        path: PathBuf,
        /// Output a structured JSON inspection report.
        #[arg(long)]
        json: bool,
    },
    /// Render a `ContextPacket` artifact without recompiling it.
    Render {
        /// Path to a `ContextPacket` JSON artifact.
        path: PathBuf,
        /// Output format for the rendered packet.
        #[arg(long, value_enum)]
        format: PacketRenderFormat,
    },
    /// Compare two `ContextPacket` artifacts semantically.
    Diff {
        /// Older `ContextPacket` JSON artifact.
        old: PathBuf,
        /// Newer `ContextPacket` JSON artifact.
        new: PathBuf,
        /// Output a structured JSON diff report.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn handle_packet(command: &PacketCommands) -> Result<()> {
    match command {
        PacketCommands::Validate { .. } => bail!("packet validate is not implemented yet"),
        PacketCommands::Inspect { .. } => bail!("packet inspect is not implemented yet"),
        PacketCommands::Render { .. } => bail!("packet render is not implemented yet"),
        PacketCommands::Diff { .. } => bail!("packet diff is not implemented yet"),
    }
}
