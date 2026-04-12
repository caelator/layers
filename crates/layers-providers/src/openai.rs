//! OpenAI-compatible model provider.

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{self, Stream, StreamExt};
use reqwest::Client;
use tracing::{debug, warn};

use layers_core::error::{LayersError, Result};
use layers_core::traits::{ModelProvider, Tokenizer};
use layers_core::types::*;

use crate::types::*;

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct OpenAiProvider {
    id: String,
    base_url: String,
    api_key: String,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(id: impl Into<String>, base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            client: Client::new(),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn build_wire_request(request: &ModelRequest, stream: bool) -> OpenAiChatRequest {
        let messages = request
            .messages
            .iter()
            .map(convert_message_to_openai)
            .collect();

        let tools = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| OpenAiTool {
                    tool_type: t.tool_type.clone(),
                    function: OpenAiFunction {
                        name: t.function.name.clone(),
                        description: t.function.description.clone(),
                        parameters: t.function.parameters.clone(),
                    },
                })
                .collect()
        });

        OpenAiChatRequest {
            model: request.model.model.clone(),
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            tools,
            stream: if stream { Some(true) } else { None },
        }
    }
}

#[async_trait::async_trait]
impl ModelProvider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        let wire = Self::build_wire_request(&request, false);
        debug!(model = %wire.model, "OpenAI complete request");

        let resp = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
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

        let oai: OpenAiChatResponse = resp
            .json()
            .await
            .map_err(|e| LayersError::Provider(format!("decode error: {e}")))?;

        convert_openai_response(oai)
    }

    fn complete_stream(
        &self,
        request: ModelRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>> {
        let wire = Self::build_wire_request(&request, true);
        let endpoint = self.endpoint();
        let api_key = self.api_key.clone();
        let client = self.client.clone();

        Box::pin(stream::once(async move {
            let resp = client
                .post(endpoint)
                .bearer_auth(&api_key)
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
                    // In a full implementation we would read the SSE byte stream,
                    // parse "data: " lines, handle "[DONE]", and yield StreamChunks.
                    // For now, yield a single empty chunk to satisfy the type.
                    warn!("OpenAI streaming not fully wired — returning empty chunk");
                    Some(Ok(StreamChunk {
                        delta_text: None,
                        delta_reasoning: None,
                        tool_call_delta: None,
                        usage: None,
                        finish_reason: Some("stop".into()),
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
        128_000
    }

    fn max_tokens(&self) -> usize {
        16_384
    }

    fn tokenizer(&self) -> Option<Arc<dyn Tokenizer>> {
        Some(Arc::new(ApproxTokenizer))
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn convert_message_to_openai(msg: &Message) -> OpenAiMessage {
    let role = match msg.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };

    let content: Option<serde_json::Value> = match &msg.content {
        MessageContent::Text(t) => Some(serde_json::Value::String(t.clone())),
        MessageContent::Parts(parts) => {
            let arr: Vec<serde_json::Value> = parts
                .iter()
                .filter_map(|p| serde_json::to_value(p).ok())
                .collect();
            Some(serde_json::Value::Array(arr))
        }
    };

    let tool_calls = msg.tool_calls.as_ref().map(|tcs| {
        tcs.iter()
            .map(|tc| OpenAiToolCall {
                id: Some(tc.id.clone()),
                call_type: Some(tc.call_type.clone()),
                function: Some(OpenAiFunctionCall {
                    name: Some(tc.function.name.clone()),
                    arguments: Some(tc.function.arguments.clone()),
                }),
                index: None,
            })
            .collect()
    });

    OpenAiMessage {
        role: role.to_string(),
        content,
        name: msg.name.clone(),
        tool_calls,
        tool_call_id: msg.tool_call_id.clone(),
    }
}

fn convert_openai_response(oai: OpenAiChatResponse) -> Result<ModelResponse> {
    let choice = oai
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| LayersError::Provider("empty choices".into()))?;

    let resp_msg = choice
        .message
        .ok_or_else(|| LayersError::Provider("missing message in choice".into()))?;

    let tool_calls = resp_msg.tool_calls.map(|tcs| {
        tcs.into_iter()
            .map(|tc| ToolCall {
                id: tc.id.unwrap_or_default(),
                call_type: tc.call_type.unwrap_or_else(|| "function".into()),
                function: FunctionCall {
                    name: tc
                        .function
                        .as_ref()
                        .and_then(|f| f.name.clone())
                        .unwrap_or_default(),
                    arguments: tc
                        .function
                        .as_ref()
                        .and_then(|f| f.arguments.clone())
                        .unwrap_or_default(),
                },
            })
            .collect()
    });

    let message = Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text(resp_msg.content.unwrap_or_default()),
        name: None,
        tool_calls,
        tool_call_id: None,
        reasoning: None,
        timestamp: None,
    };

    let usage = oai
        .usage
        .map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
        })
        .unwrap_or_default();

    Ok(ModelResponse {
        message,
        usage,
        model: oai.model,
        finish_reason: choice.finish_reason,
    })
}

// ---------------------------------------------------------------------------
// Approximate tokenizer
// ---------------------------------------------------------------------------

pub struct ApproxTokenizer;

impl Tokenizer for ApproxTokenizer {
    fn count_message_tokens(&self, messages: &[Message]) -> usize {
        messages.iter().map(|m| {
            let text_len = match &m.content {
                MessageContent::Text(t) => t.len(),
                MessageContent::Parts(_) => 100, // rough estimate
            };
            // ~4 chars per token + overhead per message
            text_len / 4 + 4
        }).sum()
    }

    fn count_tool_schema_tokens(&self, tools: &[ToolDefinition]) -> usize {
        tools.iter().map(|t| {
            let schema_str = serde_json::to_string(&t.function.parameters).unwrap_or_default();
            (t.function.name.len() + t.function.description.len() + schema_str.len()) / 4
        }).sum()
    }

    fn count_text_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }
}
