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

        let content = std::fs::read_to_string(&self.path).map_err(|e| {
            LayersError::Config(format!("failed to read {}: {e}", self.path.display()))
        })?;

        let config: LayersConfig = toml::from_str(&content).map_err(|e| {
            LayersError::Config(format!("failed to parse {}: {e}", self.path.display()))
        })?;

        Ok(config)
    }

    /// Read the config file asynchronously.
    pub async fn read_async(&self) -> Result<LayersConfig> {
        if !self.path.exists() {
            return Ok(LayersConfig::default());
        }

        let content = tokio::fs::read_to_string(&self.path).await.map_err(|e| {
            LayersError::Config(format!("failed to read {}: {e}", self.path.display()))
        })?;

        let config: LayersConfig = toml::from_str(&content).map_err(|e| {
            LayersError::Config(format!("failed to parse {}: {e}", self.path.display()))
        })?;

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

        std::fs::write(&self.path, content).map_err(|e| {
            LayersError::Config(format!("failed to write {}: {e}", self.path.display()))
        })?;

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

        tokio::fs::write(&self.path, content).await.map_err(|e| {
            LayersError::Config(format!("failed to write {}: {e}", self.path.display()))
        })?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use layers_core::config::ProviderConfig;

    #[test]
    fn read_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let store = ConfigStore::new(&path);
        let config = store.read().unwrap();
        assert_eq!(config.daemon.port, 3000);
    }

    #[tokio::test]
    async fn read_async_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let store = ConfigStore::new(&path);
        let config = store.read_async().await.unwrap();
        assert_eq!(config.daemon.port, 3000);
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layers.toml");
        let store = ConfigStore::new(&path);

        let mut config = LayersConfig::default();
        config.daemon.port = 9999;
        config.daemon.bind_address = "0.0.0.0".to_string();

        store.write(&config).unwrap();
        assert!(path.exists());

        let loaded = store.read().unwrap();
        assert_eq!(loaded.daemon.port, 9999);
        assert_eq!(loaded.daemon.bind_address, "0.0.0.0");
    }

    #[tokio::test]
    async fn write_async_and_read_async_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layers.toml");
        let store = ConfigStore::new(&path);

        let config = LayersConfig::default();
        store.write_async(&config).await.unwrap();

        let loaded = store.read_async().await.unwrap();
        assert_eq!(loaded.daemon.port, config.daemon.port);
    }

    #[test]
    fn validate_rejects_port_zero() {
        let mut config = LayersConfig::default();
        config.daemon.port = 0;
        let err = ConfigStore::validate(&config).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("port cannot be 0"), "got: {msg}");
    }

    #[test]
    fn validate_rejects_provider_with_key_but_no_models() {
        let mut config = LayersConfig::default();
        config.providers.insert(
            "test".to_string(),
            ProviderConfig {
                api_key: Some("sk-test".to_string()),
                api_base: None,
                models: vec![],
                extra: Default::default(),
            },
        );
        let err = ConfigStore::validate(&config).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no models"), "got: {msg}");
    }

    #[test]
    fn validate_accepts_valid_config() {
        let config = LayersConfig::default();
        ConfigStore::validate(&config).unwrap();
    }

    #[test]
    fn path_returns_configured_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layers.toml");
        let store = ConfigStore::new(&path);
        assert_eq!(store.path(), path);
    }
}
