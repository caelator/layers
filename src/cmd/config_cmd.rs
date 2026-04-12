//! `layers config` — display and manage Layers configuration.

use layers_store::config::ConfigStore;

use crate::config;

/// Arguments for the `layers config` command.
pub enum ConfigArgs {
    /// Display the resolved configuration (with secrets masked).
    Show,
    /// Display the path where configuration is loaded from.
    Path,
    /// Validate the configuration file.
    Validate,
}

/// Handle the `layers config` subcommand.
pub fn handle_config(args: &ConfigArgs) -> anyhow::Result<()> {
    match args {
        ConfigArgs::Show => {
            let cfg = config::load_config_with_precedence(None)?;
            let masked = config::mask_secrets(&cfg);
            let toml = toml::to_string_pretty(&masked)?;
            println!("{toml}");
            Ok(())
        }
        ConfigArgs::Path => {
            let workspace = config::workspace_root().join("layers.toml");
            let global = config::global_config_path();

            println!("Global config:  {}", global.display());
            println!("Workspace config: {}", workspace.display());

            if workspace.exists() {
                println!("Active: {}", workspace.display());
            } else if global.exists() {
                println!("Active: {}", global.display());
            } else {
                println!("Active: (defaults — no config file found)");
            }
            Ok(())
        }
        ConfigArgs::Validate => {
            let cfg = config::load_config_with_precedence(None)?;
            ConfigStore::validate(&cfg).map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("Configuration is valid.");
            Ok(())
        }
    }
}
