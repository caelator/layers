//! Anthropic Messages API provider.

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{self, Stream, StreamExt};
use reqwest::Client;
use tracing::{debug, warn};

use layers_core::error::{LayersError, Result};
use layers_core::traits::{ModelProvider, Tokenizer};
use layers_core::types::*;

use crate::types::*;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct AnthropicProvider {
    id: String,
    api_key: String,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(id: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            api_key: api_key.into(),
            client: Client::new(),
        }
    }

    fn build_wire_request(request: &ModelRequest, stream: bool) -> AnthropicRequest {
        // Extract system prompt — Anthropic takes it as a top-level field.
        let system = request.system.clone().or_else(|| {
            request.messages.iter().find_map(|m| {
                if m.role == MessageRole::System {
                    match &m.content {
                        MessageContent::Text(t) => Some(t.clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            })
        });

        let messages: Vec<AnthropicMessage> = request
            .messages
            .iter()
            .filter(|m| m.role != MessageRole::System)
            .map(convert_message_to_anthropic)
            .collect();

        let tools = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| AnthropicTool {
                    name: t.function.name.clone(),
                    description: t.function.description.clone(),
                    input_schema: t.function.parameters.clone(),
                })
                .collect()
        });

        AnthropicRequest {
            model: request.model.model.clone(),
            messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
            system,
            temperature: request.temperature,
            tools,
            stream: if stream { Some(true) } else { None },
        }
    }
}

#[async_trait::async_trait]
impl ModelProvider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        let wire = Self::build_wire_request(&request, false);
        debug!(model = %wire.model, "Anthropic complete request");

        let resp = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&wire)
            .send()
            .await
            .map_err(|e| LayersError::Provider(format!("request failed: {e}")))?;

        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(std::time::Duration::from_secs);
            return Err(LayersError::RateLimited { retry_after });
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(LayersError::Provider(format!("auth error: {status}")));
        }
        if status.is_server_error() {
            return Err(LayersError::Provider(format!("server error: {status}")));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LayersError::Provider(format!("{status}: {body}")));
        }

        let anth: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| LayersError::Provider(format!("decode error: {e}")))?;

        convert_anthropic_response(anth)
    }

    fn complete_stream(
        &self,
        request: ModelRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>> {
        let wire = Self::build_wire_request(&request, true);
        let api_key = self.api_key.clone();
        let client = self.client.clone();

        Box::pin(stream::once(async move {
            let resp = client
                .post(ANTHROPIC_API_URL)
                .header("x-api-key", &api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .json(&wire)
                .send()
                .await
                .map_err(|e| LayersError::Provider(format!("stream request failed: {e}")))?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(LayersError::Provider(format!("{status}: {body}")));
            }

            Ok(resp)
        })
        .filter_map(|res| async {
            match res {
                Err(e) => Some(Err(e)),
                Ok(_resp) => {
                    warn!("Anthropic streaming not fully wired — returning empty chunk");
                    Some(Ok(StreamChunk {
                        delta_text: None,
                        delta_reasoning: None,
                        tool_call_delta: None,
                        usage: None,
                        finish_reason: Some("end_turn".into()),
                    }))
                }
            }
        }))
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        true
    }

    fn context_window(&self) -> usize {
        200_000
    }

    fn max_tokens(&self) -> usize {
        8192
    }

    fn tokenizer(&self) -> Option<Arc<dyn Tokenizer>> {
        Some(Arc::new(crate::openai::ApproxTokenizer))
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn convert_message_to_anthropic(msg: &Message) -> AnthropicMessage {
    let role = match msg.role {
        MessageRole::User | MessageRole::Tool => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "user", // filtered above, but fallback
    };

    let content = match &msg.content {
        MessageContent::Text(t) => {
            // For tool results, wrap in tool_result block
            if let Some(tool_call_id) = &msg.tool_call_id {
                serde_json::json!([{
                    "type": "tool_result",
                    "tool_use_id": tool_call_id,
                    "content": t,
                }])
            } else {
                serde_json::Value::String(t.clone())
            }
        }
        MessageContent::Parts(parts) => {
            let arr: Vec<serde_json::Value> = parts
                .iter()
                .filter_map(|p| serde_json::to_value(p).ok())
                .collect();
            serde_json::Value::Array(arr)
        }
    };

    AnthropicMessage {
        role: role.to_string(),
        content,
    }
}

fn convert_anthropic_response(anth: AnthropicResponse) -> Result<ModelResponse> {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_parts = Vec::new();

    for block in &anth.content {
        match block {
            AnthropicContentBlock::Text { text } => {
                text_parts.push(text.clone());
            }
            AnthropicContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(ToolCall {
                    id: id.clone(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    },
                });
            }
            AnthropicContentBlock::Thinking { thinking } => {
                reasoning_parts.push(ReasoningPart {
                    text: thinking.clone(),
                    token_count: None,
                });
            }
        }
    }

    let message = Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text(text_parts.join("")),
        name: None,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        tool_call_id: None,
        reasoning: if reasoning_parts.is_empty() {
            None
        } else {
            Some(reasoning_parts)
        },
        timestamp: None,
    };

    let usage = Usage {
        prompt_tokens: anth.usage.input_tokens,
        completion_tokens: anth.usage.output_tokens,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_creation_tokens: None,
    };

    Ok(ModelResponse {
        message,
        usage,
        model: Some(anth.model),
        finish_reason: anth.stop_reason,
    })
}
