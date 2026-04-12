//! Tokenizer implementations for model families.
//!
//! Provides concrete [`Tokenizer`] implementations:
//! - [`TiktokenTokenizer`] — BPE-based counting using `tiktoken-rs` for OpenAI models
//! - [`AnthropicTokenizer`] — character-based heuristic tuned for Claude models
//! - [`GoogleTokenizer`] — character-based heuristic tuned for Gemini models
//! - [`FallbackTokenizer`] — generic ~4 chars/token

use std::sync::Arc;

use layers_core::traits::Tokenizer;
use layers_core::types::*;

use crate::capabilities::TokenizerFamily;

/// Extract text content from a message for token counting.
fn message_text(msg: &Message) -> String {
    match &msg.content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Parts(parts) => parts
            .iter()
            .map(|p| match p {
                ContentPart::Text { text } => text.clone(),
                ContentPart::ImageUrl { .. } => "[image]".into(),
                ContentPart::AudioUrl { .. } => "[audio]".into(),
                ContentPart::VideoUrl { .. } => "[video]".into(),
                ContentPart::File { .. } => "[file]".into(),
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

// ---------------------------------------------------------------------------
// Tiktoken-based tokenizer (OpenAI family)
// ---------------------------------------------------------------------------

/// BPE tokenizer backed by `tiktoken-rs`.
///
/// Uses the encoding appropriate for the model family (cl100k_base or
/// o200k_base). This provides accurate token counts for OpenAI models
/// and any OpenAI-compatible provider using the same tokenization.
pub struct TiktokenTokenizer {
    encoder: tiktoken_rs::CoreBPE,
}

impl TiktokenTokenizer {
    /// Create with the o200k_base encoding (GPT-4o, GPT-4.1, GPT-5.x, o3, etc.).
    pub fn o200k_base() -> Self {
        let encoder = tiktoken_rs::bpe_for_model("gpt-4o")
            .expect("gpt-4o tokenizer should be available")
            .clone();
        Self { encoder }
    }

    /// Create with the cl100k_base encoding (GPT-4, GPT-4-turbo, older models).
    pub fn cl100k_base() -> Self {
        let encoder = tiktoken_rs::bpe_for_model("gpt-4")
            .expect("gpt-4 tokenizer should be available")
            .clone();
        Self { encoder }
    }
}

impl Tokenizer for TiktokenTokenizer {
    fn count_message_tokens(&self, messages: &[Message]) -> usize {
        let mut total = 0;
        for msg in messages {
            // Per-message overhead: ~4 tokens (role, separators, etc.)
            total += 4;
            total += self.count_text_tokens(&message_text(msg));
            if let Some(name) = &msg.name {
                total += self.count_text_tokens(name);
            }
            if let Some(tcs) = &msg.tool_calls {
                for tc in tcs {
                    total += self.count_text_tokens(&tc.function.name);
                    total += self.count_text_tokens(&tc.function.arguments);
                    total += 3; // overhead per tool call
                }
            }
        }
        total += 2; // priming tokens for assistant reply
        total
    }

    fn count_tool_schema_tokens(&self, tools: &[ToolDefinition]) -> usize {
        let json = serde_json::to_string(tools).unwrap_or_default();
        self.encoder.encode_with_special_tokens(&json).len()
    }

    fn count_text_tokens(&self, text: &str) -> usize {
        self.encoder.encode_with_special_tokens(text).len()
    }
}

// ---------------------------------------------------------------------------
// Anthropic heuristic tokenizer
// ---------------------------------------------------------------------------

/// Anthropic tokenizer using tuned character heuristic.
///
/// Anthropic hasn't published their tokenizer, but empirical testing shows
/// roughly 3.5 chars/token for English text on Claude models.
pub struct AnthropicTokenizer;

impl Tokenizer for AnthropicTokenizer {
    fn count_message_tokens(&self, messages: &[Message]) -> usize {
        let mut total = 0;
        for msg in messages {
            total += 5; // per-message overhead
            total += self.count_text_tokens(&message_text(msg));
            if let Some(tcs) = &msg.tool_calls {
                for tc in tcs {
                    total += self.count_text_tokens(&tc.function.name);
                    total += self.count_text_tokens(&tc.function.arguments);
                    total += 5;
                }
            }
        }
        total += 2;
        total
    }

    fn count_tool_schema_tokens(&self, tools: &[ToolDefinition]) -> usize {
        let json = serde_json::to_string(tools).unwrap_or_default();
        json.len() / 3
    }

    fn count_text_tokens(&self, text: &str) -> usize {
        (text.len() as f64 / 3.5).ceil() as usize
    }
}

// ---------------------------------------------------------------------------
// Google heuristic tokenizer
// ---------------------------------------------------------------------------

/// Google tokenizer using tuned character heuristic.
///
/// Gemini models use a SentencePiece-based tokenizer. For English text,
/// roughly 4 chars/token is a reasonable approximation.
pub struct GoogleTokenizer;

impl Tokenizer for GoogleTokenizer {
    fn count_message_tokens(&self, messages: &[Message]) -> usize {
        let mut total = 0;
        for msg in messages {
            total += 4;
            total += self.count_text_tokens(&message_text(msg));
            if let Some(tcs) = &msg.tool_calls {
                for tc in tcs {
                    total += self.count_text_tokens(&tc.function.name);
                    total += self.count_text_tokens(&tc.function.arguments);
                    total += 3;
                }
            }
        }
        total += 2;
        total
    }

    fn count_tool_schema_tokens(&self, tools: &[ToolDefinition]) -> usize {
        let json = serde_json::to_string(tools).unwrap_or_default();
        json.len() / 4
    }

    fn count_text_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }
}

// ---------------------------------------------------------------------------
// Fallback tokenizer
// ---------------------------------------------------------------------------

/// Generic fallback tokenizer (~4 chars/token).
pub struct FallbackTokenizer;

impl Tokenizer for FallbackTokenizer {
    fn count_message_tokens(&self, messages: &[Message]) -> usize {
        let mut total = 0;
        for msg in messages {
            total += 4;
            total += self.count_text_tokens(&message_text(msg));
        }
        total
    }

    fn count_tool_schema_tokens(&self, tools: &[ToolDefinition]) -> usize {
        let json = serde_json::to_string(tools).unwrap_or_default();
        json.len() / 4
    }

    fn count_text_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }
}

// ---------------------------------------------------------------------------
// Factory function
// ---------------------------------------------------------------------------

/// Build the appropriate tokenizer for a given model family.
pub fn tokenizer_for_family(family: TokenizerFamily) -> Arc<dyn Tokenizer> {
    match family {
        TokenizerFamily::O200kBase => Arc::new(TiktokenTokenizer::o200k_base()),
        TokenizerFamily::Cl100kBase => Arc::new(TiktokenTokenizer::cl100k_base()),
        TokenizerFamily::Anthropic => Arc::new(AnthropicTokenizer),
        TokenizerFamily::Google => Arc::new(GoogleTokenizer),
        TokenizerFamily::Fallback => Arc::new(FallbackTokenizer),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiktoken_counts_accurately() {
        let tok = TiktokenTokenizer::o200k_base();
        let count = tok.count_text_tokens("Hello, world!");
        assert!((1..=5).contains(&count), "expected ~2 tokens, got {count}");
    }

    #[test]
    fn tiktoken_counts_messages() {
        let tok = TiktokenTokenizer::o200k_base();
        let messages = vec![Message {
            role: MessageRole::User,
            content: MessageContent::Text("Hello, world!".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        }];
        let count = tok.count_message_tokens(&messages);
        assert!((4..=20).contains(&count), "expected ~8 tokens, got {count}");
    }

    #[test]
    fn anthropic_heuristic_is_reasonable() {
        let tok = AnthropicTokenizer;
        let count = tok.count_text_tokens("Hello, world!");
        assert!((2..=8).contains(&count), "expected ~4 tokens, got {count}");
    }

    #[test]
    fn google_heuristic_is_reasonable() {
        let tok = GoogleTokenizer;
        let count = tok.count_text_tokens("Hello, world!");
        assert!((2..=6).contains(&count), "expected ~3 tokens, got {count}");
    }

    #[test]
    fn factory_returns_correct_type() {
        let _o200k = tokenizer_for_family(TokenizerFamily::O200kBase);
        let _cl100k = tokenizer_for_family(TokenizerFamily::Cl100kBase);
        let _anthropic = tokenizer_for_family(TokenizerFamily::Anthropic);
        let _google = tokenizer_for_family(TokenizerFamily::Google);
        let _fallback = tokenizer_for_family(TokenizerFamily::Fallback);
    }

    #[test]
    fn tiktoken_tool_schema_counts() {
        let tok = TiktokenTokenizer::o200k_base();
        let tools = vec![ToolDefinition {
            tool_type: "function".into(),
            function: ToolFunction {
                name: "read_file".into(),
                description: "Read a file from disk".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path" }
                    },
                    "required": ["path"]
                }),
            },
        }];
        let count = tok.count_tool_schema_tokens(&tools);
        assert!(count > 0, "expected non-zero tool schema tokens, got {count}");
    }

    #[test]
    fn tiktoken_vs_fallback_on_known_text() {
        let tik = TiktokenTokenizer::o200k_base();
        let fb = FallbackTokenizer;
        let text = "The quick brown fox jumps over the lazy dog.";
        let tik_count = tik.count_text_tokens(text);
        let fb_count = fb.count_text_tokens(text);
        assert!(tik_count > 0);
        assert!(fb_count > 0);
    }
}
