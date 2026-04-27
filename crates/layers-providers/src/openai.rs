//! OpenAI-compatible model provider.

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{self, Stream, StreamExt};
use reqwest::Client;
use tracing::{debug, warn};

use layers_core::error::{LayersError, Result};
use layers_core::traits::{ModelProvider, Tokenizer};
use layers_core::types::*;

use crate::tokenizer_impl::tokenizer_for_family;
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
    pub fn new(
        id: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            client: Client::new(),
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        )
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

        Box::pin(
            stream::once(async move {
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
            }),
        )
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        true
    }

    fn context_window(&self) -> usize {
        128_000 // Default; use model_caps() for per-model lookup
    }

    fn max_tokens(&self) -> usize {
        16_384
    }

    fn tokenizer(&self) -> Option<Arc<dyn Tokenizer>> {
        // Return o200k_base as the default; callers should use
        // `tokenizer_for_model()` for model-specific tokenizers.
        Some(tokenizer_for_family(
            crate::capabilities::TokenizerFamily::O200kBase,
        ))
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

// Note: ApproxTokenizer removed. See tokenizer_impl.rs for proper implementations.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use layers_core::types::{
        FunctionCall, Message, MessageContent, MessageRole, ModelRef, ModelRequest, ToolCall,
        ToolDefinition, ToolFunction,
    };

    use crate::types::{
        OpenAiChatResponse, OpenAiChoice, OpenAiFunctionCall, OpenAiResponseMessage,
        OpenAiToolCall, OpenAiUsage,
    };

    // -- Helpers --------------------------------------------------------------

    fn simple_request(model: &str) -> ModelRequest {
        ModelRequest {
            model: ModelRef {
                provider: "openai".into(),
                model: model.into(),
            },
            messages: vec![Message {
                role: MessageRole::User,
                content: MessageContent::Text("Hello".into()),
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

    fn request_with_system_and_tools() -> ModelRequest {
        ModelRequest {
            model: ModelRef {
                provider: "openai".into(),
                model: "gpt-4o".into(),
            },
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: MessageContent::Text("You are helpful.".into()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning: None,
                    timestamp: None,
                },
                Message {
                    role: MessageRole::User,
                    content: MessageContent::Text("What's the weather?".into()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning: None,
                    timestamp: None,
                },
            ],
            system: Some("System prompt".into()),
            tools: Some(vec![ToolDefinition {
                tool_type: "function".into(),
                function: ToolFunction {
                    name: "get_weather".into(),
                    description: "Get weather for a city".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "city": { "type": "string" }
                        }
                    }),
                },
            }]),
            temperature: Some(0.7),
            max_tokens: Some(1024),
            token_budget: None,
            thinking: None,
        }
    }

    // -- Construction ---------------------------------------------------------

    #[test]
    fn provider_construction() {
        let p = OpenAiProvider::new("openai", "https://api.openai.com", "sk-test");
        assert_eq!(p.id(), "openai");
        assert!(p.supports_tools());
        assert!(p.supports_vision());
        assert_eq!(p.context_window(), 128_000);
        assert_eq!(p.max_tokens(), 16_384);
    }

    #[test]
    fn endpoint_url() {
        let p = OpenAiProvider::new("openai", "https://api.openai.com", "sk-test");
        assert_eq!(p.endpoint(), "https://api.openai.com/v1/chat/completions");

        let p2 = OpenAiProvider::new("openai", "https://api.openai.com/", "sk-test");
        assert_eq!(p2.endpoint(), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn tokenizer_is_available() {
        let p = OpenAiProvider::new("openai", "https://api.openai.com", "sk-test");
        assert!(p.tokenizer().is_some());
    }

    // -- Request serialization ------------------------------------------------

    #[test]
    fn build_wire_request_simple() {
        let req = simple_request("gpt-4o");
        let wire = OpenAiProvider::build_wire_request(&req, false);

        assert_eq!(wire.model, "gpt-4o");
        assert_eq!(wire.messages.len(), 1);
        assert_eq!(wire.messages[0].role, "user");
        assert!(wire.stream.is_none());
        assert!(wire.tools.is_none());
        assert!(wire.temperature.is_none());
        assert!(wire.max_tokens.is_none());
    }

    #[test]
    fn build_wire_request_streaming() {
        let req = simple_request("gpt-4o");
        let wire = OpenAiProvider::build_wire_request(&req, true);
        assert_eq!(wire.stream, Some(true));
    }

    #[test]
    fn build_wire_request_with_tools() {
        let req = request_with_system_and_tools();
        let wire = OpenAiProvider::build_wire_request(&req, false);

        assert_eq!(wire.temperature, Some(0.7));
        assert_eq!(wire.max_tokens, Some(1024));
        assert_eq!(wire.messages.len(), 2);

        let tools = wire.tools.as_ref().expect("tools present");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, "get_weather");
        assert_eq!(tools[0].tool_type, "function");
    }

    #[test]
    fn wire_request_serializes_to_json() {
        let req = simple_request("gpt-4o");
        let wire = OpenAiProvider::build_wire_request(&req, false);
        let json = serde_json::to_value(&wire).expect("serialize wire request");

        assert_eq!(json["model"], "gpt-4o");
        assert!(json.get("stream").is_none());
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
    }

    // -- Message conversion ---------------------------------------------------

    #[test]
    fn convert_user_message() {
        let msg = Message {
            role: MessageRole::User,
            content: MessageContent::Text("test".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        };
        let oai = convert_message_to_openai(&msg);
        assert_eq!(oai.role, "user");
        assert_eq!(oai.content, Some(serde_json::Value::String("test".into())));
        assert!(oai.tool_calls.is_none());
        assert!(oai.tool_call_id.is_none());
    }

    #[test]
    fn convert_assistant_message_with_tool_calls() {
        let msg = Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("Let me check.".into()),
            name: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_123".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "get_weather".into(),
                    arguments: r#"{"city":"NYC"}"#.into(),
                },
            }]),
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        };
        let oai = convert_message_to_openai(&msg);
        assert_eq!(oai.role, "assistant");
        let tcs = oai.tool_calls.expect("tool calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, Some("call_123".into()));
        assert_eq!(
            tcs[0].function.as_ref().unwrap().name,
            Some("get_weather".into())
        );
    }

    #[test]
    fn convert_tool_result_message() {
        let msg = Message {
            role: MessageRole::Tool,
            content: MessageContent::Text("72°F sunny".into()),
            name: None,
            tool_calls: None,
            tool_call_id: Some("call_123".into()),
            reasoning: None,
            timestamp: None,
        };
        let oai = convert_message_to_openai(&msg);
        assert_eq!(oai.role, "tool");
        assert_eq!(oai.tool_call_id, Some("call_123".into()));
    }

    // -- Response parsing -----------------------------------------------------

    #[test]
    fn parse_simple_response() {
        let oai_resp = OpenAiChatResponse {
            id: "chatcmpl-123".into(),
            choices: vec![OpenAiChoice {
                message: Some(OpenAiResponseMessage {
                    role: Some("assistant".into()),
                    content: Some("Hello there!".into()),
                    tool_calls: None,
                }),
                delta: None,
                finish_reason: Some("stop".into()),
                index: 0,
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            model: Some("gpt-4o-2024-08-06".into()),
        };

        let resp = convert_openai_response(oai_resp).unwrap();
        assert_eq!(resp.message.content.as_text().unwrap(), "Hello there!");
        assert_eq!(resp.message.role, MessageRole::Assistant);
        assert_eq!(resp.usage.prompt_tokens, 10);
        assert_eq!(resp.usage.completion_tokens, 5);
        assert_eq!(resp.finish_reason, Some("stop".into()));
        assert_eq!(resp.model, Some("gpt-4o-2024-08-06".into()));
    }

    #[test]
    fn parse_response_with_tool_calls() {
        let oai_resp = OpenAiChatResponse {
            id: "chatcmpl-456".into(),
            choices: vec![OpenAiChoice {
                message: Some(OpenAiResponseMessage {
                    role: Some("assistant".into()),
                    content: None,
                    tool_calls: Some(vec![OpenAiToolCall {
                        id: Some("call_abc".into()),
                        call_type: Some("function".into()),
                        function: Some(OpenAiFunctionCall {
                            name: Some("get_weather".into()),
                            arguments: Some(r#"{"city":"NYC"}"#.into()),
                        }),
                        index: None,
                    }]),
                }),
                delta: None,
                finish_reason: Some("tool_calls".into()),
                index: 0,
            }],
            usage: None,
            model: None,
        };

        let resp = convert_openai_response(oai_resp).unwrap();
        let tcs = resp.message.tool_calls.as_ref().expect("tool calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_abc");
        assert_eq!(tcs[0].function.name, "get_weather");
        assert_eq!(tcs[0].function.arguments, r#"{"city":"NYC"}"#);
    }

    #[test]
    fn parse_empty_choices_is_error() {
        let oai_resp = OpenAiChatResponse {
            id: "chatcmpl-empty".into(),
            choices: vec![],
            usage: None,
            model: None,
        };

        let err = convert_openai_response(oai_resp).unwrap_err();
        match err {
            LayersError::Provider(msg) => assert!(msg.contains("empty choices")),
            other => panic!("expected Provider error, got: {other}"),
        }
    }

    #[test]
    fn parse_missing_message_is_error() {
        let oai_resp = OpenAiChatResponse {
            id: "chatcmpl-nomsg".into(),
            choices: vec![OpenAiChoice {
                message: None,
                delta: None,
                finish_reason: None,
                index: 0,
            }],
            usage: None,
            model: None,
        };

        let err = convert_openai_response(oai_resp).unwrap_err();
        match err {
            LayersError::Provider(msg) => assert!(msg.contains("missing message")),
            other => panic!("expected Provider error, got: {other}"),
        }
    }

    #[test]
    fn parse_response_no_usage_defaults_to_zero() {
        let oai_resp = OpenAiChatResponse {
            id: "chatcmpl-nousage".into(),
            choices: vec![OpenAiChoice {
                message: Some(OpenAiResponseMessage {
                    role: Some("assistant".into()),
                    content: Some("Hi".into()),
                    tool_calls: None,
                }),
                delta: None,
                finish_reason: Some("stop".into()),
                index: 0,
            }],
            usage: None,
            model: None,
        };

        let resp = convert_openai_response(oai_resp).unwrap();
        assert_eq!(resp.usage.prompt_tokens, 0);
        assert_eq!(resp.usage.completion_tokens, 0);
    }
}
