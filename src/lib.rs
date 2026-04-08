//! Layers library crate — exposes core modules for integration testing.
//!
//! The primary entry point remains `main.rs` (the binary crate).
//! This library re-exports stable public interfaces used by integration tests
//! and downstream consumers.

#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(unsafe_op_in_unsafe_fn)]
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
    clippy::module_name_repetitions,
    // These modules were written for the binary crate; exposing them as a
    // library triggers pedantic warnings that are not worth fixing in every
    // downstream module just for integration test access.
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::return_self_not_must_use
)]

pub mod config;
pub mod critical_path;
pub mod feedback;
pub mod quality;
pub mod router;
pub mod util;

#[cfg(test)]
pub mod test_support;
