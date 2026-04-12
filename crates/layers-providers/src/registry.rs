//! Provider registry — lookup, alias resolution, and fallback routing.

use std::collections::HashMap;

use tracing::{debug, warn};

use layers_core::error::{LayersError, Result};
use layers_core::traits::ModelProvider;
use layers_core::types::*;

// ---------------------------------------------------------------------------
// Aliases
// ---------------------------------------------------------------------------

fn default_aliases() -> HashMap<&'static str, (&'static str, &'static str)> {
    let mut m = HashMap::new();
    // Short name → (provider, model)
    m.insert("opus", ("anthropic", "claude-opus-4-6"));
    m.insert("sonnet", ("anthropic", "claude-sonnet-4-6"));
    m.insert("haiku", ("anthropic", "claude-haiku-4-5-20251001"));
    m.insert("gpt4o", ("openai", "gpt-4o"));
    m.insert("gpt4o-mini", ("openai", "gpt-4o-mini"));
    m.insert("o3", ("openai", "o3"));
    m.insert("gemini-pro", ("google", "gemini-2.5-pro-preview-06-05"));
    m.insert("gemini-flash", ("google", "gemini-2.5-flash-preview-05-20"));
    m
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn ModelProvider>>,
    aliases: HashMap<String, (String, String)>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        let aliases = default_aliases()
            .into_iter()
            .map(|(k, (p, m))| (k.to_string(), (p.to_string(), m.to_string())))
            .collect();

        Self {
            providers: HashMap::new(),
            aliases,
        }
    }

    /// Register a provider under its `id()`.
    pub fn register(&mut self, provider: Box<dyn ModelProvider>) {
        let id = provider.id().to_string();
        debug!(provider = %id, "registered provider");
        self.providers.insert(id, provider);
    }

    /// Look up a provider by name.
    pub fn get(&self, provider_name: &str) -> Option<&dyn ModelProvider> {
        self.providers.get(provider_name).map(|b| b.as_ref())
    }

    /// Resolve a `ModelRef` to the corresponding provider.
    pub fn resolve(&self, model_ref: &ModelRef) -> Option<&dyn ModelProvider> {
        self.get(&model_ref.provider)
    }

    /// Resolve a short alias (e.g. "opus") to a full `ModelRef`.
    pub fn resolve_alias(&self, alias: &str) -> Option<ModelRef> {
        self.aliases.get(alias).map(|(p, m)| ModelRef {
            provider: p.clone(),
            model: m.clone(),
        })
    }

    /// Add a custom alias.
    pub fn add_alias(&mut self, alias: impl Into<String>, provider: impl Into<String>, model: impl Into<String>) {
        self.aliases.insert(alias.into(), (provider.into(), model.into()));
    }

    /// Try providers in order, returning the first successful response.
    pub async fn complete_with_fallback(
        &self,
        request: &ModelRequest,
        fallbacks: &[ModelRef],
    ) -> Result<ModelResponse> {
        let mut last_err = None;

        for model_ref in fallbacks {
            let provider = match self.resolve(model_ref) {
                Some(p) => p,
                None => {
                    warn!(provider = %model_ref.provider, "provider not found, skipping");
                    continue;
                }
            };

            let req = ModelRequest {
                model: model_ref.clone(),
                messages: request.messages.clone(),
                system: request.system.clone(),
                tools: request.tools.clone(),
                temperature: request.temperature,
                max_tokens: request.max_tokens,
                token_budget: request.token_budget.clone(),
                thinking: request.thinking.clone(),
            };

            match provider.complete(req).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    warn!(provider = %model_ref.full_id(), error = %e, "provider failed, trying next");
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or(LayersError::FallbackExhausted))
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
