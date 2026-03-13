use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tracing::{debug, instrument};

use crate::error::{LlmError, Result};
use crate::provider::{LlmProvider, ResponseStream};
use crate::types::{LlmRequest, LlmResponse, Role, TokenUsage};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Claude provider.
pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Create from the `ANTHROPIC_API_KEY` environment variable.
    pub fn from_env() -> Self {
        let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");
        Self::new(api_key)
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}

// ── Anthropic request/response shapes ────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    model: String,
    usage: AnthropicUsage,
    stop_reason: Option<String>,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    #[instrument(skip(self, request), fields(model = %request.model))]
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        // Extract system prompt if present.
        let system_prompt: Option<&str> = request
            .messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.content.as_str());

        let messages: Vec<AnthropicMessage<'_>> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| AnthropicMessage {
                role: match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::System => "user", // filtered above
                },
                content: &m.content,
            })
            .collect();

        let body = AnthropicRequest {
            model: &request.model,
            messages,
            system: system_prompt,
            max_tokens: request.max_tokens.unwrap_or(1024),
            temperature: request.temperature,
            stream: false,
        };

        debug!(model = %request.model, "sending request to Anthropic");

        let resp = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::ProviderError {
                provider: "anthropic".to_string(),
                message: format!("HTTP {status}: {text}"),
            });
        }

        let parsed: AnthropicResponse = resp.json().await?;
        let content = parsed
            .content
            .into_iter()
            .filter(|c| c.kind == "text")
            .filter_map(|c| c.text)
            .collect::<Vec<_>>()
            .join("");

        Ok(LlmResponse {
            content,
            model: parsed.model,
            usage: Some(TokenUsage {
                input_tokens: parsed.usage.input_tokens,
                output_tokens: parsed.usage.output_tokens,
            }),
            stop_reason: parsed.stop_reason,
        })
    }

    async fn stream(&self, request: &LlmRequest) -> Result<ResponseStream> {
        // Streaming uses server-sent events; return a basic chunk stream.
        let system_prompt: Option<&str> = request
            .messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.content.as_str());

        let messages: Vec<AnthropicMessage<'_>> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| AnthropicMessage {
                role: match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::System => "user",
                },
                content: &m.content,
            })
            .collect();

        let body = AnthropicRequest {
            model: &request.model,
            messages,
            system: system_prompt,
            max_tokens: request.max_tokens.unwrap_or(1024),
            temperature: request.temperature,
            stream: true,
        };

        let resp = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::ProviderError {
                provider: "anthropic".to_string(),
                message: format!("HTTP {status}: {text}"),
            });
        }

        // Convert bytes stream into SSE text chunks.
        let byte_stream = resp.bytes_stream();
        let text_stream = byte_stream.filter_map(|chunk| {
            match chunk {
                Err(e) => Some(Err(LlmError::Http(e))),
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    // Parse "data: {...}" SSE lines.
                    let mut out = String::new();
                    for line in text.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                break;
                            }
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data)
                                && let Some(t) = val
                                    .get("delta")
                                    .and_then(|d| d.get("text"))
                                    .and_then(|t| t.as_str())
                            {
                                out.push_str(t);
                            }
                        }
                    }
                    if out.is_empty() { None } else { Some(Ok(out)) }
                }
            }
        });

        Ok(Box::pin(text_stream))
    }
}
