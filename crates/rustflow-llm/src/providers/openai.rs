use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tracing::{debug, instrument};

use crate::error::{LlmError, Result};
use crate::provider::{LlmProvider, ResponseStream};
use crate::types::{LlmRequest, LlmResponse, Role, TokenUsage};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// OpenAI (and compatible) provider.
pub struct OpenAiProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Create from the `OPENAI_API_KEY` environment variable.
    pub fn from_env() -> Self {
        let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
        Self::new(api_key)
    }

    /// Override the base URL for OpenAI-compatible endpoints (e.g. Azure, local).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}

// ── OpenAI request/response shapes ───────────────────────────────────────────

#[derive(Serialize)]
struct OaiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OaiRequest<'a> {
    model: &'a str,
    messages: Vec<OaiMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Deserialize)]
struct OaiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct OaiMessage2 {
    content: Option<String>,
}

#[derive(Deserialize)]
struct OaiChoice {
    message: OaiMessage2,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OaiResponse {
    model: String,
    choices: Vec<OaiChoice>,
    usage: Option<OaiUsage>,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
    }

    #[instrument(skip(self, request), fields(model = %request.model))]
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let messages: Vec<OaiMessage<'_>> = request
            .messages
            .iter()
            .map(|m| OaiMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                content: &m.content,
            })
            .collect();

        let body = OaiRequest {
            model: &request.model,
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: false,
        };

        debug!(model = %request.model, "sending request to OpenAI");

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::ProviderError {
                provider: "openai".to_string(),
                message: format!("HTTP {status}: {text}"),
            });
        }

        let parsed: OaiResponse = resp.json().await?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::ProviderError {
                provider: "openai".to_string(),
                message: "no choices returned".to_string(),
            })?;

        Ok(LlmResponse {
            content: choice.message.content.unwrap_or_default(),
            model: parsed.model,
            usage: parsed.usage.map(|u| TokenUsage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }),
            stop_reason: choice.finish_reason,
        })
    }

    async fn stream(&self, request: &LlmRequest) -> Result<ResponseStream> {
        let messages: Vec<OaiMessage<'_>> = request
            .messages
            .iter()
            .map(|m| OaiMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                content: &m.content,
            })
            .collect();

        let body = OaiRequest {
            model: &request.model,
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: true,
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::ProviderError {
                provider: "openai".to_string(),
                message: format!("HTTP {status}: {text}"),
            });
        }

        let byte_stream = resp.bytes_stream();
        let text_stream = byte_stream.filter_map(|chunk| match chunk {
            Err(e) => Some(Err(LlmError::Http(e))),
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                let mut out = String::new();
                for line in text.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            break;
                        }
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data)
                            && let Some(delta_text) = val
                                .get("choices")
                                .and_then(|c| c.get(0))
                                .and_then(|c| c.get("delta"))
                                .and_then(|d| d.get("content"))
                                .and_then(|t| t.as_str())
                        {
                            out.push_str(delta_text);
                        }
                    }
                }
                if out.is_empty() { None } else { Some(Ok(out)) }
            }
        });

        Ok(Box::pin(text_stream))
    }
}
