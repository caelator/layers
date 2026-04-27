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
    pub fn add_alias(
        &mut self,
        alias: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) {
        self.aliases
            .insert(alias.into(), (provider.into(), model.into()));
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::Arc;

    use futures::stream::{self, Stream};

    use layers_core::error::LayersError;
    use layers_core::traits::{ModelProvider, Tokenizer};
    use layers_core::types::{
        Message, MessageContent, MessageRole, ModelRef, ModelRequest, ModelResponse, StreamChunk,
        Usage,
    };

    // -- Mock provider -------------------------------------------------------

    struct MockProvider {
        name: String,
        should_fail: bool,
    }

    impl MockProvider {
        fn ok(name: &str) -> Box<dyn ModelProvider> {
            Box::new(Self {
                name: name.into(),
                should_fail: false,
            })
        }

        fn failing(name: &str) -> Box<dyn ModelProvider> {
            Box::new(Self {
                name: name.into(),
                should_fail: true,
            })
        }
    }

    #[async_trait::async_trait]
    impl ModelProvider for MockProvider {
        fn id(&self) -> &str {
            &self.name
        }

        async fn complete(&self, _request: ModelRequest) -> Result<ModelResponse> {
            if self.should_fail {
                return Err(LayersError::Provider(format!("{} mock failure", self.name)));
            }
            Ok(ModelResponse {
                message: Message {
                    role: MessageRole::Assistant,
                    content: MessageContent::Text(format!("response from {}", self.name)),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning: None,
                    timestamp: None,
                },
                usage: Usage::default(),
                model: Some(self.name.clone()),
                finish_reason: Some("stop".into()),
            })
        }

        fn complete_stream(
            &self,
            _request: ModelRequest,
        ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>> {
            Box::pin(stream::empty())
        }

        fn supports_tools(&self) -> bool {
            false
        }
        fn supports_vision(&self) -> bool {
            false
        }
        fn context_window(&self) -> usize {
            4096
        }
        fn max_tokens(&self) -> usize {
            1024
        }
        fn tokenizer(&self) -> Option<Arc<dyn Tokenizer>> {
            None
        }
    }

    // -- Helpers --------------------------------------------------------------

    fn make_request(provider: &str, model: &str) -> ModelRequest {
        ModelRequest {
            model: ModelRef {
                provider: provider.into(),
                model: model.into(),
            },
            messages: vec![Message {
                role: MessageRole::User,
                content: MessageContent::Text("hello".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning: None,
                timestamp: None,
            }],
            system: None,
            tools: None,
            temperature: None,
            max_tokens: None,
            token_budget: None,
            thinking: None,
        }
    }

    // -- Tests ----------------------------------------------------------------

    #[test]
    fn register_and_get() {
        let mut reg = ProviderRegistry::new();
        reg.register(MockProvider::ok("test-provider"));

        assert!(reg.get("test-provider").is_some());
        assert_eq!(reg.get("test-provider").unwrap().id(), "test-provider");
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn resolve_model_ref() {
        let mut reg = ProviderRegistry::new();
        reg.register(MockProvider::ok("openai"));

        let model_ref = ModelRef {
            provider: "openai".into(),
            model: "gpt-4o".into(),
        };
        assert!(reg.resolve(&model_ref).is_some());

        let missing = ModelRef {
            provider: "missing".into(),
            model: "x".into(),
        };
        assert!(reg.resolve(&missing).is_none());
    }

    #[test]
    fn default_aliases_resolve() {
        let reg = ProviderRegistry::new();

        let opus = reg.resolve_alias("opus").expect("opus alias");
        assert_eq!(opus.provider, "anthropic");
        assert_eq!(opus.model, "claude-opus-4-6");

        let gpt = reg.resolve_alias("gpt4o").expect("gpt4o alias");
        assert_eq!(gpt.provider, "openai");
        assert_eq!(gpt.model, "gpt-4o");

        let gemini = reg.resolve_alias("gemini-pro").expect("gemini-pro alias");
        assert_eq!(gemini.provider, "google");

        // All default aliases
        for alias in [
            "opus",
            "sonnet",
            "haiku",
            "gpt4o",
            "gpt4o-mini",
            "o3",
            "gemini-pro",
            "gemini-flash",
        ] {
            assert!(reg.resolve_alias(alias).is_some(), "missing alias: {alias}");
        }
    }

    #[test]
    fn unknown_alias_returns_none() {
        let reg = ProviderRegistry::new();
        assert!(reg.resolve_alias("nonexistent-alias").is_none());
        assert!(reg.resolve_alias("").is_none());
    }

    #[test]
    fn add_custom_alias() {
        let mut reg = ProviderRegistry::new();
        reg.add_alias("my-model", "local", "llama-3");

        let resolved = reg.resolve_alias("my-model").expect("custom alias");
        assert_eq!(resolved.provider, "local");
        assert_eq!(resolved.model, "llama-3");
    }

    #[test]
    fn custom_alias_overrides_default() {
        let mut reg = ProviderRegistry::new();
        reg.add_alias("opus", "custom", "my-opus");

        let resolved = reg.resolve_alias("opus").unwrap();
        assert_eq!(resolved.provider, "custom");
        assert_eq!(resolved.model, "my-opus");
    }

    #[tokio::test]
    async fn fallback_success_on_first() {
        let mut reg = ProviderRegistry::new();
        reg.register(MockProvider::ok("provider-a"));
        reg.register(MockProvider::ok("provider-b"));

        let request = make_request("provider-a", "model-a");
        let fallbacks = vec![
            ModelRef {
                provider: "provider-a".into(),
                model: "model-a".into(),
            },
            ModelRef {
                provider: "provider-b".into(),
                model: "model-b".into(),
            },
        ];

        let resp = reg
            .complete_with_fallback(&request, &fallbacks)
            .await
            .unwrap();
        let text = resp.message.content.as_text().unwrap();
        assert!(text.contains("provider-a"));
    }

    #[tokio::test]
    async fn fallback_success_on_second_after_first_fails() {
        let mut reg = ProviderRegistry::new();
        reg.register(MockProvider::failing("provider-a"));
        reg.register(MockProvider::ok("provider-b"));

        let request = make_request("provider-a", "model-a");
        let fallbacks = vec![
            ModelRef {
                provider: "provider-a".into(),
                model: "model-a".into(),
            },
            ModelRef {
                provider: "provider-b".into(),
                model: "model-b".into(),
            },
        ];

        let resp = reg
            .complete_with_fallback(&request, &fallbacks)
            .await
            .unwrap();
        let text = resp.message.content.as_text().unwrap();
        assert!(text.contains("provider-b"));
    }

    #[tokio::test]
    async fn fallback_all_fail_returns_last_error() {
        let mut reg = ProviderRegistry::new();
        reg.register(MockProvider::failing("provider-a"));
        reg.register(MockProvider::failing("provider-b"));

        let request = make_request("provider-a", "model-a");
        let fallbacks = vec![
            ModelRef {
                provider: "provider-a".into(),
                model: "model-a".into(),
            },
            ModelRef {
                provider: "provider-b".into(),
                model: "model-b".into(),
            },
        ];

        let err = reg
            .complete_with_fallback(&request, &fallbacks)
            .await
            .unwrap_err();
        match err {
            LayersError::Provider(msg) => assert!(msg.contains("provider-b")),
            other => panic!("expected Provider error, got: {other}"),
        }
    }

    #[tokio::test]
    async fn fallback_empty_list_returns_exhausted() {
        let reg = ProviderRegistry::new();
        let request = make_request("x", "y");

        let err = reg.complete_with_fallback(&request, &[]).await.unwrap_err();
        assert!(matches!(err, LayersError::FallbackExhausted));
    }

    #[tokio::test]
    async fn fallback_skips_unregistered_provider() {
        let mut reg = ProviderRegistry::new();
        reg.register(MockProvider::ok("provider-b"));

        let request = make_request("missing", "model");
        let fallbacks = vec![
            ModelRef {
                provider: "missing".into(),
                model: "model".into(),
            },
            ModelRef {
                provider: "provider-b".into(),
                model: "model-b".into(),
            },
        ];

        let resp = reg
            .complete_with_fallback(&request, &fallbacks)
            .await
            .unwrap();
        let text = resp.message.content.as_text().unwrap();
        assert!(text.contains("provider-b"));
    }

    #[test]
    fn default_impl() {
        let reg = ProviderRegistry::default();
        // Default should have aliases but no providers
        assert!(reg.resolve_alias("opus").is_some());
        assert!(reg.get("anthropic").is_none());
    }
}
