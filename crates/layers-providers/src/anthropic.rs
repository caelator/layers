//! Anthropic Messages API provider.

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

        Box::pin(
            stream::once(async move {
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
        200_000
    }

    fn max_tokens(&self) -> usize {
        8192
    }

    fn tokenizer(&self) -> Option<Arc<dyn Tokenizer>> {
        Some(tokenizer_for_family(
            crate::capabilities::TokenizerFamily::Anthropic,
        ))
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use layers_core::types::{
        Message, MessageContent, MessageRole, ModelRef, ModelRequest, ToolDefinition, ToolFunction,
    };

    use crate::types::{AnthropicContentBlock, AnthropicResponse, AnthropicUsage};

    // -- Helpers --------------------------------------------------------------

    fn simple_request(model: &str) -> ModelRequest {
        ModelRequest {
            model: ModelRef {
                provider: "anthropic".into(),
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

    // -- Construction ---------------------------------------------------------

    #[test]
    fn provider_construction() {
        let p = AnthropicProvider::new("anthropic", "sk-ant-test");
        assert_eq!(p.id(), "anthropic");
        assert!(p.supports_tools());
        assert!(p.supports_vision());
        assert_eq!(p.context_window(), 200_000);
        assert_eq!(p.max_tokens(), 8192);
    }

    #[test]
    fn tokenizer_is_available() {
        let p = AnthropicProvider::new("anthropic", "sk-ant-test");
        assert!(p.tokenizer().is_some());
    }

    // -- Request serialization ------------------------------------------------

    #[test]
    fn build_wire_request_simple() {
        let req = simple_request("claude-sonnet-4-6");
        let wire = AnthropicProvider::build_wire_request(&req, false);

        assert_eq!(wire.model, "claude-sonnet-4-6");
        assert_eq!(wire.max_tokens, 4096); // default when None
        assert_eq!(wire.messages.len(), 1);
        assert_eq!(wire.messages[0].role, "user");
        assert!(wire.system.is_none());
        assert!(wire.stream.is_none());
        assert!(wire.tools.is_none());
    }

    #[test]
    fn build_wire_request_streaming() {
        let req = simple_request("claude-sonnet-4-6");
        let wire = AnthropicProvider::build_wire_request(&req, true);
        assert_eq!(wire.stream, Some(true));
    }

    #[test]
    fn build_wire_request_extracts_system_prompt() {
        let req = ModelRequest {
            model: ModelRef {
                provider: "anthropic".into(),
                model: "claude-sonnet-4-6".into(),
            },
            messages: vec![Message {
                role: MessageRole::User,
                content: MessageContent::Text("Hi".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning: None,
                timestamp: None,
            }],
            system: Some("Be concise.".into()),
            tools: None,
            temperature: Some(0.5),
            max_tokens: Some(2048),
            token_budget: None,
            thinking: None,
        };

        let wire = AnthropicProvider::build_wire_request(&req, false);
        assert_eq!(wire.system, Some("Be concise.".into()));
        assert_eq!(wire.temperature, Some(0.5));
        assert_eq!(wire.max_tokens, 2048);
    }

    #[test]
    fn build_wire_request_system_from_message_fallback() {
        let req = ModelRequest {
            model: ModelRef {
                provider: "anthropic".into(),
                model: "claude-sonnet-4-6".into(),
            },
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: MessageContent::Text("System from message".into()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning: None,
                    timestamp: None,
                },
                Message {
                    role: MessageRole::User,
                    content: MessageContent::Text("Hi".into()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning: None,
                    timestamp: None,
                },
            ],
            system: None,
            tools: None,
            temperature: None,
            max_tokens: None,
            token_budget: None,
            thinking: None,
        };

        let wire = AnthropicProvider::build_wire_request(&req, false);
        assert_eq!(wire.system, Some("System from message".into()));
        // System messages should be filtered out of the messages array
        assert!(wire.messages.iter().all(|m| m.role != "system"));
    }

    #[test]
    fn build_wire_request_with_tools() {
        let req = ModelRequest {
            model: ModelRef {
                provider: "anthropic".into(),
                model: "claude-sonnet-4-6".into(),
            },
            messages: vec![Message {
                role: MessageRole::User,
                content: MessageContent::Text("Use the tool".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning: None,
                timestamp: None,
            }],
            system: None,
            tools: Some(vec![ToolDefinition {
                tool_type: "function".into(),
                function: ToolFunction {
                    name: "get_weather".into(),
                    description: "Get weather".into(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            }]),
            temperature: None,
            max_tokens: None,
            token_budget: None,
            thinking: None,
        };

        let wire = AnthropicProvider::build_wire_request(&req, false);
        let tools = wire.tools.expect("tools present");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "get_weather");
        assert_eq!(tools[0].description, "Get weather");
    }

    #[test]
    fn wire_request_serializes_with_anthropic_fields() {
        let req = simple_request("claude-sonnet-4-6");
        let wire = AnthropicProvider::build_wire_request(&req, false);
        let json = serde_json::to_value(&wire).expect("serialize");

        assert_eq!(json["model"], "claude-sonnet-4-6");
        assert_eq!(json["max_tokens"], 4096);
        assert!(json.get("stream").is_none());
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
        let anth = convert_message_to_anthropic(&msg);
        assert_eq!(anth.role, "user");
        assert_eq!(anth.content, serde_json::Value::String("test".into()));
    }

    #[test]
    fn convert_assistant_message() {
        let msg = Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("response".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        };
        let anth = convert_message_to_anthropic(&msg);
        assert_eq!(anth.role, "assistant");
    }

    #[test]
    fn convert_tool_result_wraps_in_tool_result_block() {
        let msg = Message {
            role: MessageRole::Tool,
            content: MessageContent::Text("72°F".into()),
            name: None,
            tool_calls: None,
            tool_call_id: Some("toolu_123".into()),
            reasoning: None,
            timestamp: None,
        };
        let anth = convert_message_to_anthropic(&msg);
        assert_eq!(anth.role, "user");

        // Should be wrapped in a tool_result content block
        let arr = anth.content.as_array().expect("array content");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "tool_result");
        assert_eq!(arr[0]["tool_use_id"], "toolu_123");
        assert_eq!(arr[0]["content"], "72°F");
    }

    #[test]
    fn tool_role_maps_to_user() {
        let msg = Message {
            role: MessageRole::Tool,
            content: MessageContent::Text("result".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        };
        let anth = convert_message_to_anthropic(&msg);
        assert_eq!(anth.role, "user");
    }

    // -- Response parsing -----------------------------------------------------

    #[test]
    fn parse_text_response() {
        let anth_resp = AnthropicResponse {
            id: "msg_123".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![AnthropicContentBlock::Text {
                text: "Hello!".into(),
            }],
            model: "claude-sonnet-4-6".into(),
            stop_reason: Some("end_turn".into()),
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
        };

        let resp = convert_anthropic_response(anth_resp).unwrap();
        assert_eq!(resp.message.content.as_text().unwrap(), "Hello!");
        assert_eq!(resp.message.role, MessageRole::Assistant);
        assert_eq!(resp.usage.prompt_tokens, 10);
        assert_eq!(resp.usage.completion_tokens, 5);
        assert_eq!(resp.model, Some("claude-sonnet-4-6".into()));
        assert_eq!(resp.finish_reason, Some("end_turn".into()));
    }

    #[test]
    fn parse_tool_use_response() {
        let anth_resp = AnthropicResponse {
            id: "msg_456".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![
                AnthropicContentBlock::Text {
                    text: "Let me check the weather.".into(),
                },
                AnthropicContentBlock::ToolUse {
                    id: "toolu_abc".into(),
                    name: "get_weather".into(),
                    input: serde_json::json!({"city": "NYC"}),
                },
            ],
            model: "claude-sonnet-4-6".into(),
            stop_reason: Some("tool_use".into()),
            usage: AnthropicUsage {
                input_tokens: 20,
                output_tokens: 15,
            },
        };

        let resp = convert_anthropic_response(anth_resp).unwrap();
        assert!(resp.message.content.as_text().unwrap().contains("weather"));
        let tcs = resp.message.tool_calls.as_ref().expect("tool calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "toolu_abc");
        assert_eq!(tcs[0].function.name, "get_weather");
        assert_eq!(tcs[0].call_type, "function");
        let args: serde_json::Value =
            serde_json::from_str(&tcs[0].function.arguments).expect("parse args");
        assert_eq!(args["city"], "NYC");
    }

    #[test]
    fn parse_thinking_response() {
        let anth_resp = AnthropicResponse {
            id: "msg_789".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![
                AnthropicContentBlock::Thinking {
                    thinking: "Let me think about this...".into(),
                },
                AnthropicContentBlock::Text {
                    text: "The answer is 42.".into(),
                },
            ],
            model: "claude-opus-4-6".into(),
            stop_reason: Some("end_turn".into()),
            usage: AnthropicUsage {
                input_tokens: 30,
                output_tokens: 25,
            },
        };

        let resp = convert_anthropic_response(anth_resp).unwrap();
        let reasoning = resp.message.reasoning.as_ref().expect("reasoning");
        assert_eq!(reasoning.len(), 1);
        assert!(reasoning[0].text.contains("think about this"));
        assert_eq!(resp.message.content.as_text().unwrap(), "The answer is 42.");
    }

    #[test]
    fn parse_response_no_tool_calls_is_none() {
        let anth_resp = AnthropicResponse {
            id: "msg_notool".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![AnthropicContentBlock::Text {
                text: "Just text.".into(),
            }],
            model: "claude-sonnet-4-6".into(),
            stop_reason: Some("end_turn".into()),
            usage: AnthropicUsage::default(),
        };

        let resp = convert_anthropic_response(anth_resp).unwrap();
        assert!(resp.message.tool_calls.is_none());
        assert!(resp.message.reasoning.is_none());
    }
}
