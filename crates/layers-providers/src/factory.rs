//! Provider factory — constructs `ModelProvider` instances from `AuthProfile` data.
//!
//! This is the bridge between persisted auth credentials (in SQLite via
//! `AuthProfileStore`) and the live provider instances registered in
//! `ProviderRegistry`. At daemon startup, `bootstrap_from_store` reads all
//! profiles and registers the corresponding providers.

use tracing::{info, warn};

use layers_core::error::{LayersError, Result};
use layers_core::traits::AuthProfileStore;
use layers_core::types::AuthProfile;

use crate::anthropic::AnthropicProvider;
use crate::google::GoogleProvider;
use crate::openai::OpenAiProvider;
use crate::registry::ProviderRegistry;

// ---------------------------------------------------------------------------
// Provider construction from AuthProfile
// ---------------------------------------------------------------------------

/// Construct a `Box<dyn ModelProvider>` from an `AuthProfile`.
///
/// The profile's `provider` field selects the concrete adapter:
/// - `"openai"` → `OpenAiProvider` (also used for any OpenAI-compatible endpoint)
/// - `"anthropic"` → `AnthropicProvider`
/// - `"google"` → `GoogleProvider`
///
/// Any other value is treated as OpenAI-compatible if an `api_base` is set,
/// allowing custom/local providers.
pub fn build_provider(profile: &AuthProfile) -> Result<Box<dyn layers_core::traits::ModelProvider>> {
    let api_key = profile
        .api_key
        .as_deref()
        .unwrap_or("");

    match profile.provider.as_str() {
        "openai" => {
            let base = profile
                .api_base
                .as_deref()
                .unwrap_or("https://api.openai.com");
            Ok(Box::new(OpenAiProvider::new(
                &profile.name,
                base,
                api_key,
            )))
        }
        "anthropic" => Ok(Box::new(AnthropicProvider::new(
            &profile.name,
            api_key,
        ))),
        "google" => Ok(Box::new(GoogleProvider::new(
            &profile.name,
            api_key,
        ))),
        other => {
            // Custom provider — must have an api_base, treated as OpenAI-compatible.
            let base = profile.api_base.as_deref().ok_or_else(|| {
                LayersError::Config(format!(
                    "custom provider '{other}' requires an api_base URL"
                ))
            })?;
            info!(
                provider = other,
                base = base,
                "constructing custom OpenAI-compatible provider"
            );
            Ok(Box::new(OpenAiProvider::new(&profile.name, base, api_key)))
        }
    }
}

// ---------------------------------------------------------------------------
// Registry bootstrap
// ---------------------------------------------------------------------------

/// Read all auth profiles from the store and register their providers.
///
/// Skips profiles that fail to construct (logs a warning). Returns the number
/// of providers successfully registered.
pub async fn bootstrap_from_store(
    registry: &mut ProviderRegistry,
    store: &dyn AuthProfileStore,
) -> Result<usize> {
    let profiles = store.list_profiles(None).await?;
    let mut count = 0;

    for profile in &profiles {
        match build_provider(profile) {
            Ok(provider) => {
                info!(
                    name = %profile.name,
                    provider = %profile.provider,
                    "registered provider from auth profile"
                );
                registry.register(provider);
                count += 1;
            }
            Err(e) => {
                warn!(
                    name = %profile.name,
                    provider = %profile.provider,
                    error = %e,
                    "skipping auth profile — failed to construct provider"
                );
            }
        }
    }

    Ok(count)
}

/// Register providers from TOML config data (fallback / seed path).
///
/// This allows bootstrapping from the `[providers]` section of `layers.toml`
/// when no auth profiles exist yet in the store.
pub fn bootstrap_from_config(
    registry: &mut ProviderRegistry,
    providers: &[(String, layers_core::config::ProviderConfig)],
) -> usize {
    let mut count = 0;

    for (name, config) in providers {
        let api_key = match config.api_key.as_deref() {
            Some(k) => k,
            None => {
                warn!(name = %name, "skipping config provider — no api_key");
                continue;
            }
        };

        let provider: Box<dyn layers_core::traits::ModelProvider> = match name.as_str() {
            "openai" => {
                let base = config
                    .api_base
                    .as_deref()
                    .unwrap_or("https://api.openai.com");
                Box::new(OpenAiProvider::new(name, base, api_key))
            }
            "anthropic" => Box::new(AnthropicProvider::new(name, api_key)),
            "google" => Box::new(GoogleProvider::new(name, api_key)),
            other => {
                let base = match config.api_base.as_deref() {
                    Some(b) => b,
                    None => {
                        warn!(
                            name = %other,
                            "skipping custom config provider — no api_base"
                        );
                        continue;
                    }
                };
                Box::new(OpenAiProvider::new(name, base, api_key))
            }
        };

        info!(name = %name, "registered provider from config");
        registry.register(provider);
        count += 1;
    }

    count
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use layers_core::config::ProviderConfig;
    use std::collections::HashMap;
    use chrono::Utc;

    #[test]
    fn build_provider_openai() {
        let profile = AuthProfile {
            name: "openai-main".into(),
            provider: "openai".into(),
            api_key: Some("sk-test".into()),
            api_base: Some("https://api.openai.com".into()),
            models: vec!["gpt-4o".into()],
            created_at: Utc::now(),
        };
        let provider = build_provider(&profile).expect("build openai provider");
        assert_eq!(provider.id(), "openai-main");
    }

    #[test]
    fn build_provider_anthropic() {
        let profile = AuthProfile {
            name: "anthropic-main".into(),
            provider: "anthropic".into(),
            api_key: Some("sk-ant-test".into()),
            api_base: None,
            models: vec![],
            created_at: Utc::now(),
        };
        let provider = build_provider(&profile).expect("build anthropic provider");
        assert_eq!(provider.id(), "anthropic-main");
    }

    #[test]
    fn build_provider_google() {
        let profile = AuthProfile {
            name: "google-main".into(),
            provider: "google".into(),
            api_key: Some("AIza...test".into()),
            api_base: None,
            models: vec![],
            created_at: Utc::now(),
        };
        let provider = build_provider(&profile).expect("build google provider");
        assert_eq!(provider.id(), "google-main");
    }

    #[test]
    fn build_provider_custom_requires_api_base() {
        let profile = AuthProfile {
            name: "custom-llama".into(),
            provider: "ollama".into(),
            api_key: None,
            api_base: None,
            models: vec![],
            created_at: Utc::now(),
        };
        assert!(build_provider(&profile).is_err());
    }

    #[test]
    fn build_provider_custom_with_api_base() {
        let profile = AuthProfile {
            name: "custom-llama".into(),
            provider: "ollama".into(),
            api_key: Some("no-key".into()),
            api_base: Some("http://localhost:11434".into()),
            models: vec![],
            created_at: Utc::now(),
        };
        let provider = build_provider(&profile).expect("build custom provider");
        assert_eq!(provider.id(), "custom-llama");
    }

    #[test]
    fn bootstrap_from_config_registers_providers() {
        let mut registry = ProviderRegistry::new();

        let providers = vec![
            (
                "openai".into(),
                ProviderConfig {
                    api_key: Some("sk-test".into()),
                    api_base: None,
                    models: vec!["gpt-4o".into()],
                    extra: HashMap::new(),
                },
            ),
            (
                "anthropic".into(),
                ProviderConfig {
                    api_key: Some("sk-ant-test".into()),
                    api_base: None,
                    models: vec![],
                    extra: HashMap::new(),
                },
            ),
        ];

        let count = bootstrap_from_config(&mut registry, &providers);
        assert_eq!(count, 2);
        assert!(registry.get("openai").is_some());
        assert!(registry.get("anthropic").is_some());
    }

    #[test]
    fn bootstrap_from_config_skips_no_key() {
        let mut registry = ProviderRegistry::new();

        let providers = vec![(
            "openai".into(),
            ProviderConfig {
                api_key: None,
                api_base: None,
                models: vec![],
                extra: HashMap::new(),
            },
        )];

        let count = bootstrap_from_config(&mut registry, &providers);
        assert_eq!(count, 0);
    }
}
