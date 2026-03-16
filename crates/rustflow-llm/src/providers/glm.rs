use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;
use tracing::{debug, instrument};

use crate::error::{LlmError, Result};
use crate::provider::{LlmProvider, ResponseStream};
use crate::types::{LlmRequest, LlmResponse, Role, TokenUsage};

const DEFAULT_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";
const DEFAULT_MODEL: &str = "glm-4-flash";

/// ZhipuAI GLM provider (OpenAI-compatible API).
///
/// Configure via environment variables:
/// - `GLM_API_KEY`  — required
/// - `GLM_MODEL`    — optional, defaults to `glm-4-flash`
pub struct GlmProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl GlmProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: DEFAULT_MODEL.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Create from environment variables `GLM_API_KEY` and optionally `GLM_MODEL`.
    pub fn from_env() -> Self {
        let api_key = std::env::var("GLM_API_KEY").expect("GLM_API_KEY must be set");
        let model =
            std::env::var("GLM_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Self {
            api_key,
            model,
            base_url: DEFAULT_BASE_URL.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Override the base URL (useful for testing or self-hosted deployments).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Override the default model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Resolve the model: use the request's model if non-empty, otherwise fall back to the
    /// provider default (set via `GLM_MODEL` / `with_model()`).
    fn resolve_model<'a>(&'a self, request: &'a LlmRequest) -> &'a str {
        if request.model.is_empty() {
            &self.model
        } else {
            &request.model
        }
    }
}

// ── GLM request/response shapes (OpenAI-compatible) ──────────────────────────

#[derive(Serialize)]
struct GlmMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct GlmRequest<'a> {
    model: &'a str,
    messages: Vec<GlmMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Deserialize)]
struct GlmUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct GlmMessageBody {
    content: Option<String>,
}

#[derive(Deserialize)]
struct GlmChoice {
    message: GlmMessageBody,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GlmResponse {
    model: String,
    choices: Vec<GlmChoice>,
    usage: Option<GlmUsage>,
}

fn role_str(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

#[async_trait]
impl LlmProvider for GlmProvider {
    fn name(&self) -> &str {
        "glm"
    }

    #[instrument(skip(self, request), fields(model = %request.model))]
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let model = self.resolve_model(request);
        let messages: Vec<GlmMessage<'_>> = request
            .messages
            .iter()
            .map(|m| GlmMessage {
                role: role_str(&m.role),
                content: &m.content,
            })
            .collect();

        let body = GlmRequest {
            model,
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream: false,
        };

        debug!(model, "sending request to GLM");

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
                provider: "glm".to_string(),
                message: format!("HTTP {status}: {text}"),
            });
        }

        let parsed: GlmResponse = resp.json().await?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::ProviderError {
                provider: "glm".to_string(),
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
        let model = self.resolve_model(request);
        let messages: Vec<GlmMessage<'_>> = request
            .messages
            .iter()
            .map(|m| GlmMessage {
                role: role_str(&m.role),
                content: &m.content,
            })
            .collect();

        let body = GlmRequest {
            model,
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
                provider: "glm".to_string(),
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
