#![deny(warnings)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![deny(unreachable_pub)]
#![allow(unreachable_pub)]
#![allow(
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::struct_excessive_bools
)]

//! `proveit` — executable proof gate for feature completion.

use anyhow::Result;
use clap::Parser;
use layers_proveit::{Cli, run};

fn main() -> Result<()> {
    let cli = Cli::parse();
    run(cli)
}
