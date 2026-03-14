use async_trait::async_trait;

use rustflow_llm::error::{LlmError, Result};
use rustflow_llm::gateway::LlmGateway;
use rustflow_llm::provider::{LlmProvider, ResponseStream};
use rustflow_llm::types::{LlmRequest, LlmResponse, TokenUsage};

/// A fake provider for testing gateway routing.
struct FakeProvider {
    provider_name: String,
}

impl FakeProvider {
    fn new(name: &str) -> Self {
        Self {
            provider_name: name.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for FakeProvider {
    fn name(&self) -> &str {
        &self.provider_name
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: format!("response from {}", self.provider_name),
            model: request.model.clone(),
            usage: Some(TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            }),
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn stream(&self, _request: &LlmRequest) -> Result<ResponseStream> {
        Err(LlmError::StreamingNotSupported {
            provider: self.provider_name.clone(),
        })
    }
}

#[test]
fn test_gateway_new_is_empty() {
    let gw = LlmGateway::new();
    assert!(gw.providers().is_empty());
}

#[test]
fn test_gateway_register_provider() {
    let mut gw = LlmGateway::new();
    gw.register(FakeProvider::new("fake-1"));
    assert_eq!(gw.providers().len(), 1);
    assert!(gw.providers().contains(&"fake-1"));
}

#[test]
fn test_gateway_register_multiple() {
    let mut gw = LlmGateway::new();
    gw.register(FakeProvider::new("openai"))
        .register(FakeProvider::new("anthropic"));
    assert_eq!(gw.providers().len(), 2);
}

#[tokio::test]
async fn test_gateway_complete_routes_to_provider() {
    let mut gw = LlmGateway::new();
    gw.register(FakeProvider::new("test-provider"));
    let req = LlmRequest::new("gpt-4", vec![]);
    let resp = gw.complete("test-provider", &req).await.unwrap();
    assert_eq!(resp.content, "response from test-provider");
    assert_eq!(resp.model, "gpt-4");
}

#[tokio::test]
async fn test_gateway_complete_default() {
    let mut gw = LlmGateway::new();
    gw.register(FakeProvider::new("default-prov"));
    let req = LlmRequest::new("model-x", vec![]);
    let resp = gw.complete_default(&req).await.unwrap();
    assert_eq!(resp.content, "response from default-prov");
}

#[tokio::test]
async fn test_gateway_complete_unknown_provider() {
    let gw = LlmGateway::new();
    let req = LlmRequest::new("gpt-4", vec![]);
    let err = gw.complete("nonexistent", &req).await.unwrap_err();
    assert!(matches!(err, LlmError::ProviderNotFound { .. }));
}

#[tokio::test]
async fn test_gateway_complete_default_no_providers() {
    let gw = LlmGateway::new();
    let req = LlmRequest::new("gpt-4", vec![]);
    let err = gw.complete_default(&req).await.unwrap_err();
    assert!(matches!(err, LlmError::ProviderNotFound { .. }));
}

#[tokio::test]
async fn test_gateway_stream_not_supported() {
    let mut gw = LlmGateway::new();
    gw.register(FakeProvider::new("fake"));
    let req = LlmRequest::new("gpt-4", vec![]);
    let result = gw.stream("fake", &req).await;
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(matches!(err, LlmError::StreamingNotSupported { .. }));
}
