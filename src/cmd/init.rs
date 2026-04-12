//! `layers init` — bootstrap a new Layers workspace.
//!
//! Creates the standard directory structure and a starter `layers.toml`
//! in the current directory (or `LAYERS_WORKSPACE_ROOT`).

use std::fs;
use std::path::PathBuf;

use layers_core::config::LayersConfig;
use layers_store::config::ConfigStore;

/// Arguments for the `layers init` command.
pub struct InitArgs {
    /// Force overwrite existing files.
    pub force: bool,
    /// Path to initialize (defaults to workspace root).
    pub path: Option<PathBuf>,
}

/// Run workspace initialization.
pub fn handle_init(args: &InitArgs) -> anyhow::Result<()> {
    let root = args
        .path
        .clone()
        .unwrap_or_else(crate::config::workspace_root);

    if root.exists() {
        // ok
    } else {
        fs::create_dir_all(&root)?;
        println!("created {}", root.display());
    }

    // Create directory structure
    let dirs = [
        "memoryport",
        "memoryport/council-runs",
    ];
    for dir in &dirs {
        let path = root.join(dir);
        if path.exists() {
            println!("exists  {}/", path.display());
        } else {
            fs::create_dir_all(&path)?;
            println!("created {}/", path.display());
        }
    }

    // Create starter layers.toml if not present
    let config_path = root.join("layers.toml");
    if config_path.exists() && !args.force {
        println!("exists  {} (use --force to overwrite)", config_path.display());
    } else {
        let starter = starter_config();
        let store = ConfigStore::new(&config_path);
        store.write(&starter)?;
        println!("created {}", config_path.display());
    }

    // Create .gitignore for memoryport if not present
    let gitignore = root.join("memoryport/.gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, "# Layers memory data\n*.jsonl\n")?;
        println!("created {}", gitignore.display());
    }

    // Create empty curated memory file if not present
    let curated = root.join("memoryport/curated-memory.jsonl");
    if !curated.exists() {
        fs::write(&curated, "")?;
        println!("created {}", curated.display());
    }

    println!();
    println!("Layers workspace initialized at {}", root.display());
    println!("Edit {} to configure providers and channels.", config_path.display());

    Ok(())
}

/// Generate a starter `layers.toml` configuration.
fn starter_config() -> LayersConfig {
    LayersConfig::default()
}
