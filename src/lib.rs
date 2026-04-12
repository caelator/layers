//! Layers library crate — re-exports workspace member crates for integration
//! tests and downstream consumers while the primary entry point remains the CLI
//! binary in `main.rs`.

pub use layers_core::{config, types, util};
pub use layers_council::{
    cmd, council, critical_path, feedback, graph, memory, quality, router, technician, uc,
};
pub use layers_plugins::plugins;
pub use layers_proveit::proveit;
