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
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::return_self_not_must_use
)]

#[path = "../../../src/config.rs"]
pub mod config;
#[path = "../../../src/types.rs"]
pub mod types;
#[path = "../../../src/util.rs"]
pub mod util;

