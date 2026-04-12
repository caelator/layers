//! Per-model capability descriptors.
//!
//! Each model has known limits for context window, max output tokens, and
//! feature support. This module provides a lookup from `(provider, model_id)`
//! to a [`ModelCapabilities`] struct, with sensible defaults when a model
//! isn't explicitly listed.

use std::collections::HashMap;

/// Capabilities and limits for a specific model.
#[derive(Debug, Clone)]
pub struct ModelCapabilities {
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_reasoning: bool,
    pub tokenizer_family: TokenizerFamily,
}

/// Which tokenizer encoding to use for a model family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenizerFamily {
    /// OpenAI cl100k_base (GPT-4, GPT-4o, GPT-4o-mini, o1, o3, etc.)
    Cl100kBase,
    /// OpenAI o200k_base (GPT-4.1, GPT-5.x, newer models)
    O200kBase,
    /// Anthropic-like approximation (no public BPE; use character heuristic)
    Anthropic,
    /// Google Gemini approximation
    Google,
    /// Generic fallback (~4 chars/token)
    Fallback,
}

/// Lookup table for model capabilities.
pub struct ModelCapabilityRegistry {
    /// Keyed by lowercase "provider/model_id".
    entries: HashMap<String, ModelCapabilities>,
}

impl ModelCapabilityRegistry {
    /// Build the registry with known models.
    pub fn new() -> Self {
        let mut entries = HashMap::new();

        // --- OpenAI / OpenAI-compatible ---
        let openai_gpt4o = ModelCapabilities {
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            tokenizer_family: TokenizerFamily::O200kBase,
        };
        insert(&mut entries, "openai", "gpt-4o", &openai_gpt4o);
        insert(&mut entries, "openai", "gpt-4o-mini", &ModelCapabilities {
            max_output_tokens: 16_384,
            ..openai_gpt4o.clone()
        });
        insert(&mut entries, "openai", "gpt-4.1", &ModelCapabilities {
            context_window: 1_047_576,
            max_output_tokens: 32_768,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            tokenizer_family: TokenizerFamily::O200kBase,
        });
        insert(&mut entries, "openai", "gpt-4.1-mini", &ModelCapabilities {
            context_window: 1_047_576,
            max_output_tokens: 32_768,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            tokenizer_family: TokenizerFamily::O200kBase,
        });
        insert(&mut entries, "openai", "gpt-4.1-nano", &ModelCapabilities {
            context_window: 1_047_576,
            max_output_tokens: 32_768,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            tokenizer_family: TokenizerFamily::O200kBase,
        });
        insert(&mut entries, "openai", "o3", &ModelCapabilities {
            context_window: 200_000,
            max_output_tokens: 100_000,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::O200kBase,
        });
        insert(&mut entries, "openai", "o4-mini", &ModelCapabilities {
            context_window: 200_000,
            max_output_tokens: 100_000,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::O200kBase,
        });
        // GPT-5.x
        insert(&mut entries, "openai", "gpt-5", &ModelCapabilities {
            context_window: 400_000,
            max_output_tokens: 32_768,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            tokenizer_family: TokenizerFamily::O200kBase,
        });
        insert(&mut entries, "openai", "gpt-5.4", &ModelCapabilities {
            context_window: 400_000,
            max_output_tokens: 32_768,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            tokenizer_family: TokenizerFamily::O200kBase,
        });

        // --- ZAI (OpenAI-compatible, uses same tokenizers) ---
        insert(&mut entries, "zai", "glm-5", &ModelCapabilities {
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::O200kBase,
        });
        insert(&mut entries, "zai", "glm-5.1", &ModelCapabilities {
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::O200kBase,
        });

        // --- OpenRouter (varies by model, default to O200kBase) ---
        insert(&mut entries, "openrouter", "auto", &ModelCapabilities {
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: false,
            tokenizer_family: TokenizerFamily::O200kBase,
        });

        // --- Groq (OpenAI-compatible) ---
        insert(&mut entries, "groq", "llama-3.3-70b-versatile", &ModelCapabilities {
            context_window: 128_000,
            max_output_tokens: 32_768,
            supports_tools: true,
            supports_vision: false,
            supports_reasoning: false,
            tokenizer_family: TokenizerFamily::Fallback,
        });

        // --- DeepSeek (OpenAI-compatible) ---
        insert(&mut entries, "deepseek", "deepseek-chat", &ModelCapabilities {
            context_window: 64_000,
            max_output_tokens: 8_192,
            supports_tools: true,
            supports_vision: false,
            supports_reasoning: false,
            tokenizer_family: TokenizerFamily::Fallback,
        });
        insert(&mut entries, "deepseek", "deepseek-reasoner", &ModelCapabilities {
            context_window: 64_000,
            max_output_tokens: 8_192,
            supports_tools: false,
            supports_vision: false,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::Fallback,
        });

        // --- Anthropic ---
        insert(&mut entries, "anthropic", "claude-opus-4-6", &ModelCapabilities {
            context_window: 200_000,
            max_output_tokens: 32_000,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::Anthropic,
        });
        insert(&mut entries, "anthropic", "claude-sonnet-4-6", &ModelCapabilities {
            context_window: 200_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::Anthropic,
        });
        insert(&mut entries, "anthropic", "claude-sonnet-4-5-20250514", &ModelCapabilities {
            context_window: 200_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::Anthropic,
        });
        insert(&mut entries, "anthropic", "claude-haiku-4-5-20251001", &ModelCapabilities {
            context_window: 200_000,
            max_output_tokens: 8_192,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::Anthropic,
        });

        // --- Google ---
        insert(&mut entries, "google", "gemini-2.5-pro-preview-06-05", &ModelCapabilities {
            context_window: 1_048_576,
            max_output_tokens: 65_536,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::Google,
        });
        insert(&mut entries, "google", "gemini-2.5-flash-preview-05-20", &ModelCapabilities {
            context_window: 1_048_576,
            max_output_tokens: 65_536,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::Google,
        });
        insert(&mut entries, "google", "gemini-3-flash-preview", &ModelCapabilities {
            context_window: 1_000_000,
            max_output_tokens: 65_536,
            supports_tools: true,
            supports_vision: true,
            supports_reasoning: true,
            tokenizer_family: TokenizerFamily::Google,
        });

        Self { entries }
    }

    /// Look up capabilities for a specific provider+model.
    ///
    /// Falls back to a default based on provider family if the exact model
    /// isn't found.
    pub fn get(&self, provider: &str, model: &str) -> ModelCapabilities {
        let key = format!("{}/{}", provider.to_lowercase(), model.to_lowercase());

        if let Some(caps) = self.entries.get(&key) {
            return caps.clone();
        }

        // Provider-level defaults
        match provider.to_lowercase().as_str() {
            "anthropic" => ModelCapabilities {
                context_window: 200_000,
                max_output_tokens: 8_192,
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: true,
                tokenizer_family: TokenizerFamily::Anthropic,
            },
            "google" => ModelCapabilities {
                context_window: 1_000_000,
                max_output_tokens: 8_192,
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: false,
                tokenizer_family: TokenizerFamily::Google,
            },
            // All OpenAI-compatible providers
            _ => ModelCapabilities {
                context_window: 128_000,
                max_output_tokens: 16_384,
                supports_tools: true,
                supports_vision: true,
                supports_reasoning: false,
                tokenizer_family: TokenizerFamily::O200kBase,
            },
        }
    }

    /// Look up by model ref string like "openai/gpt-4o".
    pub fn get_by_ref(&self, model_ref: &str) -> ModelCapabilities {
        let parts: Vec<&str> = model_ref.splitn(2, '/').collect();
        match parts.as_slice() {
            [provider, model] => self.get(provider, model),
            _ => self.get("", model_ref),
        }
    }
}

fn insert(
    entries: &mut HashMap<String, ModelCapabilities>,
    provider: &str,
    model: &str,
    caps: &ModelCapabilities,
) {
    entries.insert(format!("{}/{}", provider, model), caps.clone());
}

impl Default for ModelCapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_model_returns_exact_caps() {
        let reg = ModelCapabilityRegistry::new();
        let caps = reg.get("openai", "gpt-4o");
        assert_eq!(caps.context_window, 128_000);
        assert_eq!(caps.max_output_tokens, 16_384);
        assert!(caps.supports_tools);
    }

    #[test]
    fn unknown_openai_model_gets_default() {
        let reg = ModelCapabilityRegistry::new();
        let caps = reg.get("openai", "gpt-99-quantum");
        assert_eq!(caps.context_window, 128_000);
        assert_eq!(caps.tokenizer_family, TokenizerFamily::O200kBase);
    }

    #[test]
    fn unknown_anthropic_model_gets_anthropic_default() {
        let reg = ModelCapabilityRegistry::new();
        let caps = reg.get("anthropic", "claude-future-7");
        assert_eq!(caps.context_window, 200_000);
        assert_eq!(caps.tokenizer_family, TokenizerFamily::Anthropic);
    }

    #[test]
    fn by_ref_works() {
        let reg = ModelCapabilityRegistry::new();
        let caps = reg.get_by_ref("anthropic/claude-opus-4-6");
        assert_eq!(caps.context_window, 200_000);
    }

    #[test]
    fn case_insensitive() {
        let reg = ModelCapabilityRegistry::new();
        let caps = reg.get("OpenAI", "GPT-4o");
        assert_eq!(caps.context_window, 128_000);
    }
}
