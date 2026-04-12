//! Google Generative AI provider.

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{self, Stream, StreamExt};
use reqwest::Client;
use tracing::{debug, warn};

use layers_core::error::{LayersError, Result};
use layers_core::traits::{ModelProvider, Tokenizer};
use layers_core::types::*;

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
            .map(|m| convert_message_to_google(m))
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
        Some(Arc::new(crate::openai::ApproxTokenizer))
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
