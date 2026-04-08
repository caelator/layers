//! Core `proveit` implementation.

mod artifact_store;
mod git;
mod manifest;
mod runner;
mod service;
mod types;

pub use service::run;
pub use types::Cli;
