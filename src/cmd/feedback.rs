//! `layers feedback` — record a route correction.
//!
//! Usage:
//!   layers feedback "task text" --predicted `memory_only` --actual `graph_only`
//!   layers feedback "task text" --predicted both --actual neither
//!
//! Corrections are written to ~/.layers/route-corrections.jsonl and used
//! by the routing algorithm to demote repeatedly-wrong routes.

use anyhow::{Context, Result};
use clap::Parser;

use crate::router::{self, Route, RouteCorrection};

/// Record that Layers chose the wrong route for a task.
#[derive(Parser, Debug)]
#[command(author, version)]
pub struct FeedbackArgs {
    /// The task text that was originally classified.
    pub task: String,

    /// The route the system originally predicted.
    #[arg(long)]
    pub predicted: RouteArg,

    /// The route that was actually correct.
    #[arg(long)]
    pub actual: RouteArg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteArg {
    Neither,
    MemoryOnly,
    GraphOnly,
    Both,
}

impl std::str::FromStr for RouteArg {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.to_lowercase().as_str() {
            "neither" => Ok(RouteArg::Neither),
            "memory_only" | "memoryonly" => Ok(RouteArg::MemoryOnly),
            "graph_only" | "graphonly" => Ok(RouteArg::GraphOnly),
            "both" => Ok(RouteArg::Both),
            _ => Err(format!(
                "unknown route '{s}': expected one of neither, memory_only, graph_only, both"
            )),
        }
    }
}

impl std::fmt::Display for RouteArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouteArg::Neither => write!(f, "neither"),
            RouteArg::MemoryOnly => write!(f, "memory_only"),
            RouteArg::GraphOnly => write!(f, "graph_only"),
            RouteArg::Both => write!(f, "both"),
        }
    }
}

impl From<RouteArg> for Route {
    fn from(arg: RouteArg) -> Route {
        match arg {
            RouteArg::Neither => Route::Neither,
            RouteArg::MemoryOnly => Route::MemoryOnly,
            RouteArg::GraphOnly => Route::GraphOnly,
            RouteArg::Both => Route::Both,
        }
    }
}

pub fn handle_feedback(args: &FeedbackArgs) -> Result<()> {
    let correction = RouteCorrection::new(
        args.task.clone(),
        args.predicted.into(),
        args.actual.into(),
    );

    router::record_correction(&correction)
        .with_context(|| format!("failed to write to {}", router::corrections_path().display()))?;

    // Reload the in-process cache so classify() picks up the new correction immediately.
    router::reload_corrections();

    println!(
        "Recorded correction: predicted={} → actual={}  [{} corrections on file]",
        args.predicted,
        args.actual,
        router::load_corrections().len(),
    );
    Ok(())
}
