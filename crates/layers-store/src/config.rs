//! TOML configuration store.
//!
//! Reads and writes `layers.toml` files with validation.

use std::path::{Path, PathBuf};

use layers_core::config::LayersConfig;
use layers_core::error::{LayersError, Result};

/// Manages reading and writing of layers.toml configuration files.
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    /// Create a config store for the given path.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Return the config file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read and parse the config file.
    pub fn read(&self) -> Result<LayersConfig> {
        if !self.path.exists() {
            return Ok(LayersConfig::default());
        }

        let content = std::fs::read_to_string(&self.path)
            .map_err(|e| LayersError::Config(format!("failed to read {}: {e}", self.path.display())))?;

        let config: LayersConfig = toml::from_str(&content)
            .map_err(|e| LayersError::Config(format!("failed to parse {}: {e}", self.path.display())))?;

        Ok(config)
    }

    /// Read the config file asynchronously.
    pub async fn read_async(&self) -> Result<LayersConfig> {
        if !self.path.exists() {
            return Ok(LayersConfig::default());
        }

        let content = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(|e| LayersError::Config(format!("failed to read {}: {e}", self.path.display())))?;

        let config: LayersConfig = toml::from_str(&content)
            .map_err(|e| LayersError::Config(format!("failed to parse {}: {e}", self.path.display())))?;

        Ok(config)
    }

    /// Write the full config to disk, replacing the existing file.
    pub fn write(&self, config: &LayersConfig) -> Result<()> {
        let content = toml::to_string_pretty(config)
            .map_err(|e| LayersError::Config(format!("failed to serialize config: {e}")))?;

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| LayersError::Config(format!("failed to create config dir: {e}")))?;
        }

        std::fs::write(&self.path, content)
            .map_err(|e| LayersError::Config(format!("failed to write {}: {e}", self.path.display())))?;

        Ok(())
    }

    /// Write the config asynchronously.
    pub async fn write_async(&self, config: &LayersConfig) -> Result<()> {
        let content = toml::to_string_pretty(config)
            .map_err(|e| LayersError::Config(format!("failed to serialize config: {e}")))?;

        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| LayersError::Config(format!("failed to create config dir: {e}")))?;
        }

        tokio::fs::write(&self.path, content)
            .await
            .map_err(|e| LayersError::Config(format!("failed to write {}: {e}", self.path.display())))?;

        Ok(())
    }

    /// Validate a config without writing it.
    pub fn validate(config: &LayersConfig) -> Result<()> {
        // Ensure port is in valid range
        if config.daemon.port == 0 {
            return Err(LayersError::Config("daemon port cannot be 0".into()));
        }

        // Validate provider configs have at least one model if api_key is set
        for (name, provider) in &config.providers {
            if provider.api_key.is_some() && provider.models.is_empty() {
                return Err(LayersError::Config(format!(
                    "provider '{name}' has an API key but no models configured"
                )));
            }
        }

        // Validate bindings reference known agents
        for binding in &config.bindings {
            if binding.agent.is_empty() {
                return Err(LayersError::Config("binding has empty agent name".into()));
            }
            if binding.channel.is_empty() {
                return Err(LayersError::Config("binding has empty channel name".into()));
            }
        }

        Ok(())
    }
}
