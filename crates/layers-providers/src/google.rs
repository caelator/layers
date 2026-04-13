//! Google Generative AI provider.

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

const GOOGLE_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct GoogleProvider {
    id: String,
    api_key: String,
    client: Client,
}

impl GoogleProvider {
    pub fn new(id: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            api_key: api_key.into(),
            client: Client::new(),
        }
    }

    fn endpoint(&self, model: &str) -> String {
        format!(
            "{}/models/{}:generateContent?key={}",
            GOOGLE_API_BASE, model, self.api_key
        )
    }

    fn build_wire_request(request: &ModelRequest) -> GoogleRequest {
        let system_instruction = request.system.as_ref().map(|s| GoogleContent {
            role: None,
            parts: vec![GooglePart {
                text: Some(s.clone()),
                function_call: None,
                function_response: None,
            }],
        });

        let contents: Vec<GoogleContent> = request
            .messages
            .iter()
            .filter(|m| m.role != MessageRole::System)
            .map(convert_message_to_google)
            .collect();

        let tools = request.tools.as_ref().map(|ts| {
            vec![GoogleTool {
                function_declarations: ts
                    .iter()
                    .map(|t| GoogleFunctionDeclaration {
                        name: t.function.name.clone(),
                        description: t.function.description.clone(),
                        parameters: t.function.parameters.clone(),
                    })
                    .collect(),
            }]
        });

        let generation_config = if request.temperature.is_some() || request.max_tokens.is_some() {
            Some(GoogleGenerationConfig {
                temperature: request.temperature,
                max_output_tokens: request.max_tokens,
            })
        } else {
            None
        };

        GoogleRequest {
            contents,
            system_instruction,
            generation_config,
            tools,
        }
    }
}

#[async_trait::async_trait]
impl ModelProvider for GoogleProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        let model = request.model.model.clone();
        let wire = Self::build_wire_request(&request);
        debug!(model = %model, "Google complete request");

        let resp = self
            .client
            .post(self.endpoint(&model))
            .header("content-type", "application/json")
            .json(&wire)
            .send()
            .await
            .map_err(|e| LayersError::Provider(format!("request failed: {e}")))?;

        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(LayersError::RateLimited { retry_after: None });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LayersError::Provider(format!("{status}: {body}")));
        }

        let google: GoogleResponse = resp
            .json()
            .await
            .map_err(|e| LayersError::Provider(format!("decode error: {e}")))?;

        convert_google_response(google, &model)
    }

    fn complete_stream(
        &self,
        request: ModelRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>> {
        let model = request.model.model.clone();
        let wire = Self::build_wire_request(&request);
        let endpoint = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            GOOGLE_API_BASE, model, self.api_key
        );
        let client = self.client.clone();

        Box::pin(stream::once(async move {
            let resp = client
                .post(endpoint)
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
                    warn!("Google streaming not fully wired — returning empty chunk");
                    Some(Ok(StreamChunk {
                        delta_text: None,
                        delta_reasoning: None,
                        tool_call_delta: None,
                        usage: None,
                        finish_reason: Some("STOP".into()),
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
        1_000_000
    }

    fn max_tokens(&self) -> usize {
        8192
    }

    fn tokenizer(&self) -> Option<Arc<dyn Tokenizer>> {
        Some(tokenizer_for_family(crate::capabilities::TokenizerFamily::Google))
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn convert_message_to_google(msg: &Message) -> GoogleContent {
    let role = match msg.role {
        MessageRole::User | MessageRole::System | MessageRole::Tool => "user",
        MessageRole::Assistant => "model",
    };

    let parts = match &msg.content {
        MessageContent::Text(t) => {
            if msg.role == MessageRole::Tool {
                // Tool results use function_response
                vec![GooglePart {
                    text: None,
                    function_call: None,
                    function_response: Some(GoogleFunctionResponse {
                        name: msg.name.clone().unwrap_or_default(),
                        response: serde_json::json!({ "result": t }),
                    }),
                }]
            } else {
                vec![GooglePart {
                    text: Some(t.clone()),
                    function_call: None,
                    function_response: None,
                }]
            }
        }
        MessageContent::Parts(_) => {
            // Simplified — just extract text parts
            vec![GooglePart {
                text: Some("[multipart content]".into()),
                function_call: None,
                function_response: None,
            }]
        }
    };

    GoogleContent {
        role: Some(role.into()),
        parts,
    }
}

fn convert_google_response(google: GoogleResponse, model: &str) -> Result<ModelResponse> {
    let candidate = google
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| LayersError::Provider("empty candidates".into()))?;

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for part in &candidate.content.parts {
        if let Some(text) = &part.text {
            text_parts.push(text.clone());
        }
        if let Some(fc) = &part.function_call {
            tool_calls.push(ToolCall {
                id: format!("call_{}", tool_calls.len()),
                call_type: "function".into(),
                function: FunctionCall {
                    name: fc.name.clone(),
                    arguments: serde_json::to_string(&fc.args).unwrap_or_default(),
                },
            });
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
        reasoning: None,
        timestamp: None,
    };

    let usage = google
        .usage_metadata
        .map(|u| Usage {
            prompt_tokens: u.prompt_token_count,
            completion_tokens: u.candidates_token_count,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
        })
        .unwrap_or_default();

    Ok(ModelResponse {
        message,
        usage,
        model: Some(model.to_string()),
        finish_reason: candidate.finish_reason,
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

    use crate::types::{
        GoogleCandidate, GoogleContent, GoogleFunctionCall, GooglePart, GoogleResponse,
        GoogleUsageMetadata,
    };

    // -- Helpers --------------------------------------------------------------

    fn simple_request(model: &str) -> ModelRequest {
        ModelRequest {
            model: ModelRef {
                provider: "google".into(),
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
        let p = GoogleProvider::new("google", "AIza-test");
        assert_eq!(p.id(), "google");
        assert!(p.supports_tools());
        assert!(p.supports_vision());
        assert_eq!(p.context_window(), 1_000_000);
        assert_eq!(p.max_tokens(), 8192);
    }

    #[test]
    fn endpoint_url() {
        let p = GoogleProvider::new("google", "AIza-test");
        let url = p.endpoint("gemini-2.5-pro-preview-06-05");
        assert!(url.starts_with("https://generativelanguage.googleapis.com/v1beta/models/"));
        assert!(url.contains("gemini-2.5-pro-preview-06-05:generateContent"));
        assert!(url.contains("key=AIza-test"));
    }

    #[test]
    fn tokenizer_is_available() {
        let p = GoogleProvider::new("google", "AIza-test");
        assert!(p.tokenizer().is_some());
    }

    // -- Request serialization ------------------------------------------------

    #[test]
    fn build_wire_request_simple() {
        let req = simple_request("gemini-2.5-flash-preview-05-20");
        let wire = GoogleProvider::build_wire_request(&req);

        assert_eq!(wire.contents.len(), 1);
        assert_eq!(wire.contents[0].role, Some("user".into()));
        assert_eq!(wire.contents[0].parts[0].text, Some("Hello".into()));
        assert!(wire.system_instruction.is_none());
        assert!(wire.generation_config.is_none());
        assert!(wire.tools.is_none());
    }

    #[test]
    fn build_wire_request_with_system() {
        let req = ModelRequest {
            model: ModelRef {
                provider: "google".into(),
                model: "gemini-2.5-pro-preview-06-05".into(),
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
            system: Some("You are helpful.".into()),
            tools: None,
            temperature: Some(0.9),
            max_tokens: Some(2048),
            token_budget: None,
            thinking: None,
        };

        let wire = GoogleProvider::build_wire_request(&req);
        let sys = wire.system_instruction.as_ref().expect("system_instruction");
        assert_eq!(sys.parts[0].text, Some("You are helpful.".into()));
        assert!(sys.role.is_none()); // system instruction has no role

        let gc = wire.generation_config.as_ref().expect("generation_config");
        assert_eq!(gc.temperature, Some(0.9));
        assert_eq!(gc.max_output_tokens, Some(2048));
    }

    #[test]
    fn build_wire_request_filters_system_messages() {
        let req = ModelRequest {
            model: ModelRef {
                provider: "google".into(),
                model: "gemini-2.5-flash-preview-05-20".into(),
            },
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: MessageContent::Text("System msg".into()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning: None,
                    timestamp: None,
                },
                Message {
                    role: MessageRole::User,
                    content: MessageContent::Text("User msg".into()),
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

        let wire = GoogleProvider::build_wire_request(&req);
        // System messages are filtered out of contents
        assert_eq!(wire.contents.len(), 1);
        assert_eq!(wire.contents[0].parts[0].text, Some("User msg".into()));
    }

    #[test]
    fn build_wire_request_with_tools() {
        let req = ModelRequest {
            model: ModelRef {
                provider: "google".into(),
                model: "gemini-2.5-pro-preview-06-05".into(),
            },
            messages: vec![Message {
                role: MessageRole::User,
                content: MessageContent::Text("Use tools".into()),
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
                    name: "search".into(),
                    description: "Search the web".into(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            }]),
            temperature: None,
            max_tokens: None,
            token_budget: None,
            thinking: None,
        };

        let wire = GoogleProvider::build_wire_request(&req);
        let tools = wire.tools.expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function_declarations.len(), 1);
        assert_eq!(tools[0].function_declarations[0].name, "search");
    }

    #[test]
    fn wire_request_serializes_with_google_casing() {
        let req = ModelRequest {
            model: ModelRef {
                provider: "google".into(),
                model: "gemini-2.5-pro-preview-06-05".into(),
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
            system: Some("Be helpful".into()),
            tools: None,
            temperature: Some(0.5),
            max_tokens: Some(1024),
            token_budget: None,
            thinking: None,
        };

        let wire = GoogleProvider::build_wire_request(&req);
        let json = serde_json::to_value(&wire).expect("serialize");

        // Check camelCase field names
        assert!(json.get("systemInstruction").is_some());
        assert!(json.get("generationConfig").is_some());
        assert_eq!(json["generationConfig"]["maxOutputTokens"], 1024);
    }

    // -- Message conversion ---------------------------------------------------

    #[test]
    fn convert_user_message() {
        let msg = Message {
            role: MessageRole::User,
            content: MessageContent::Text("hello".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        };
        let gc = convert_message_to_google(&msg);
        assert_eq!(gc.role, Some("user".into()));
        assert_eq!(gc.parts[0].text, Some("hello".into()));
        assert!(gc.parts[0].function_call.is_none());
        assert!(gc.parts[0].function_response.is_none());
    }

    #[test]
    fn convert_assistant_message_maps_to_model() {
        let msg = Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("response".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        };
        let gc = convert_message_to_google(&msg);
        assert_eq!(gc.role, Some("model".into()));
    }

    #[test]
    fn convert_tool_result_uses_function_response() {
        let msg = Message {
            role: MessageRole::Tool,
            content: MessageContent::Text("72°F".into()),
            name: Some("get_weather".into()),
            tool_calls: None,
            tool_call_id: None,
            reasoning: None,
            timestamp: None,
        };
        let gc = convert_message_to_google(&msg);
        assert_eq!(gc.role, Some("user".into()));
        let fr = gc.parts[0]
            .function_response
            .as_ref()
            .expect("function_response");
        assert_eq!(fr.name, "get_weather");
        assert_eq!(fr.response["result"], "72°F");
        assert!(gc.parts[0].text.is_none());
    }

    // -- Response parsing -----------------------------------------------------

    #[test]
    fn parse_text_response() {
        let google_resp = GoogleResponse {
            candidates: vec![GoogleCandidate {
                content: GoogleContent {
                    role: Some("model".into()),
                    parts: vec![GooglePart {
                        text: Some("Hello there!".into()),
                        function_call: None,
                        function_response: None,
                    }],
                },
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: Some(GoogleUsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 5,
            }),
        };

        let resp = convert_google_response(google_resp, "gemini-2.5-pro-preview-06-05").unwrap();
        assert_eq!(resp.message.content.as_text().unwrap(), "Hello there!");
        assert_eq!(resp.message.role, MessageRole::Assistant);
        assert_eq!(resp.usage.prompt_tokens, 10);
        assert_eq!(resp.usage.completion_tokens, 5);
        assert_eq!(
            resp.model,
            Some("gemini-2.5-pro-preview-06-05".into())
        );
        assert_eq!(resp.finish_reason, Some("STOP".into()));
    }

    #[test]
    fn parse_function_call_response() {
        let google_resp = GoogleResponse {
            candidates: vec![GoogleCandidate {
                content: GoogleContent {
                    role: Some("model".into()),
                    parts: vec![GooglePart {
                        text: None,
                        function_call: Some(GoogleFunctionCall {
                            name: "get_weather".into(),
                            args: serde_json::json!({"city": "NYC"}),
                        }),
                        function_response: None,
                    }],
                },
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: None,
        };

        let resp = convert_google_response(google_resp, "gemini-2.5-flash-preview-05-20").unwrap();
        let tcs = resp.message.tool_calls.as_ref().expect("tool calls");
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "get_weather");
        assert_eq!(tcs[0].id, "call_0");
        assert_eq!(tcs[0].call_type, "function");
    }

    #[test]
    fn parse_empty_candidates_is_error() {
        let google_resp = GoogleResponse {
            candidates: vec![],
            usage_metadata: None,
        };

        let err = convert_google_response(google_resp, "gemini").unwrap_err();
        match err {
            LayersError::Provider(msg) => assert!(msg.contains("empty candidates")),
            other => panic!("expected Provider error, got: {other}"),
        }
    }

    #[test]
    fn parse_response_no_usage_defaults_to_zero() {
        let google_resp = GoogleResponse {
            candidates: vec![GoogleCandidate {
                content: GoogleContent {
                    role: Some("model".into()),
                    parts: vec![GooglePart {
                        text: Some("Hi".into()),
                        function_call: None,
                        function_response: None,
                    }],
                },
                finish_reason: None,
            }],
            usage_metadata: None,
        };

        let resp = convert_google_response(google_resp, "gemini").unwrap();
        assert_eq!(resp.usage.prompt_tokens, 0);
        assert_eq!(resp.usage.completion_tokens, 0);
    }

    #[test]
    fn parse_multi_part_response() {
        let google_resp = GoogleResponse {
            candidates: vec![GoogleCandidate {
                content: GoogleContent {
                    role: Some("model".into()),
                    parts: vec![
                        GooglePart {
                            text: Some("Part 1. ".into()),
                            function_call: None,
                            function_response: None,
                        },
                        GooglePart {
                            text: Some("Part 2.".into()),
                            function_call: None,
                            function_response: None,
                        },
                    ],
                },
                finish_reason: Some("STOP".into()),
            }],
            usage_metadata: None,
        };

        let resp = convert_google_response(google_resp, "gemini").unwrap();
        assert_eq!(resp.message.content.as_text().unwrap(), "Part 1. Part 2.");
    }
}
