use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tracing::{debug, instrument};

use crate::error::{LlmError, Result};
use crate::provider::{LlmProvider, ResponseStream};
use crate::types::{LlmRequest, LlmResponse, Role};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Ollama local model provider.
pub struct OllamaProvider {
    base_url: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ── Ollama request/response shapes ───────────────────────────────────────────

#[derive(Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaMessageResponse {
    content: String,
}

#[derive(Deserialize)]
struct OllamaResponse {
    model: String,
    message: OllamaMessageResponse,
    done_reason: Option<String>,
}

#[derive(Deserialize)]
struct OllamaStreamChunk {
    message: OllamaMessageResponse,
    done: bool,
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    #[instrument(skip(self, request), fields(model = %request.model))]
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let messages: Vec<OllamaMessage<'_>> = request
            .messages
            .iter()
            .map(|m| OllamaMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                content: &m.content,
            })
            .collect();

        let options = if request.temperature.is_some() || request.max_tokens.is_some() {
            Some(OllamaOptions {
                temperature: request.temperature,
                num_predict: request.max_tokens,
            })
        } else {
            None
        };

        let body = OllamaRequest {
            model: &request.model,
            messages,
            stream: false,
            options,
        };

        debug!(model = %request.model, "sending request to Ollama");

        let resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::ProviderError {
                provider: "ollama".to_string(),
                message: format!("HTTP {status}: {text}"),
            });
        }

        let parsed: OllamaResponse = resp.json().await?;

        Ok(LlmResponse {
            content: parsed.message.content,
            model: parsed.model,
            usage: None,
            stop_reason: parsed.done_reason,
        })
    }

    async fn stream(&self, request: &LlmRequest) -> Result<ResponseStream> {
        let messages: Vec<OllamaMessage<'_>> = request
            .messages
            .iter()
            .map(|m| OllamaMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                content: &m.content,
            })
            .collect();

        let options = if request.temperature.is_some() || request.max_tokens.is_some() {
            Some(OllamaOptions {
                temperature: request.temperature,
                num_predict: request.max_tokens,
            })
        } else {
            None
        };

        let body = OllamaRequest {
            model: &request.model,
            messages,
            stream: true,
            options,
        };

        let resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::ProviderError {
                provider: "ollama".to_string(),
                message: format!("HTTP {status}: {text}"),
            });
        }

        // Ollama streams newline-delimited JSON objects.
        let byte_stream = resp.bytes_stream();
        let text_stream = byte_stream.filter_map(|chunk| match chunk {
            Err(e) => Some(Err(LlmError::Http(e))),
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                let mut out = String::new();
                for line in text.lines() {
                    if let Ok(parsed) = serde_json::from_str::<OllamaStreamChunk>(line) {
                        out.push_str(&parsed.message.content);
                        if parsed.done {
                            break;
                        }
                    }
                }
                if out.is_empty() { None } else { Some(Ok(out)) }
            }
        });

        Ok(Box::pin(text_stream))
    }
}
