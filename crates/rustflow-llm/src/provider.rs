use async_trait::async_trait;
use std::pin::Pin;
use tokio_stream::Stream;

use crate::error::Result;
use crate::types::{LlmRequest, LlmResponse};

/// A pinned, boxed async stream of response chunks for streaming completions.
pub type ResponseStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

/// Trait implemented by every LLM backend (Anthropic, OpenAI, Ollama, etc.).
#[async_trait]
pub trait LlmProvider: Send + Sync + 'static {
    /// The unique name of this provider, e.g. "anthropic".
    fn name(&self) -> &str;

    /// Send a non-streaming completion request and return the full response.
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse>;

    /// Send a streaming completion request, returning a stream of text chunks.
    async fn stream(&self, request: &LlmRequest) -> Result<ResponseStream>;
}
