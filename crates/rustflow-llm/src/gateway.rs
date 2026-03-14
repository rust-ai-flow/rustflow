use std::collections::HashMap;
use std::sync::Arc;

use tracing::{debug, instrument};

use crate::error::{LlmError, Result};
use crate::provider::{LlmProvider, ResponseStream};
use crate::types::{LlmRequest, LlmResponse};

/// Routes LLM requests to the appropriate registered provider.
pub struct LlmGateway {
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    default_provider: Option<String>,
}

impl LlmGateway {
    /// Create an empty gateway.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            default_provider: None,
        }
    }

    /// Register a provider. The first registered provider becomes the default.
    pub fn register<P: LlmProvider>(&mut self, provider: P) -> &mut Self {
        let name = provider.name().to_string();
        if self.default_provider.is_none() {
            self.default_provider = Some(name.clone());
        }
        self.providers.insert(name, Arc::new(provider));
        self
    }

    /// Set the default provider by name.
    pub fn set_default(&mut self, provider_name: impl Into<String>) -> &mut Self {
        self.default_provider = Some(provider_name.into());
        self
    }

    /// Look up a provider by name.
    fn get_provider(&self, name: &str) -> Result<Arc<dyn LlmProvider>> {
        self.providers
            .get(name)
            .cloned()
            .ok_or_else(|| LlmError::ProviderNotFound {
                provider: name.to_string(),
            })
    }

    /// Return the default provider.
    fn default_provider(&self) -> Result<Arc<dyn LlmProvider>> {
        let name = self
            .default_provider
            .as_deref()
            .ok_or_else(|| LlmError::ProviderNotFound {
                provider: "<default>".to_string(),
            })?;
        self.get_provider(name)
    }

    /// Send a completion request to a named provider.
    #[instrument(skip(self, request), fields(provider, model = %request.model))]
    pub async fn complete(&self, provider_name: &str, request: &LlmRequest) -> Result<LlmResponse> {
        debug!(provider = %provider_name, "routing completion request");
        self.get_provider(provider_name)?.complete(request).await
    }

    /// Send a completion request to the default provider.
    pub async fn complete_default(&self, request: &LlmRequest) -> Result<LlmResponse> {
        self.default_provider()?.complete(request).await
    }

    /// Send a streaming request to a named provider.
    pub async fn stream(
        &self,
        provider_name: &str,
        request: &LlmRequest,
    ) -> Result<ResponseStream> {
        debug!(provider = %provider_name, "routing stream request");
        self.get_provider(provider_name)?.stream(request).await
    }

    /// List all registered provider names.
    pub fn providers(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }
}

impl Default for LlmGateway {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::LlmProvider;
    use crate::types::{LlmResponse, TokenUsage};
    use async_trait::async_trait;

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
    fn test_gateway_first_provider_becomes_default() {
        let mut gw = LlmGateway::new();
        gw.register(FakeProvider::new("first"));
        gw.register(FakeProvider::new("second"));
        // The default should be "first"
        assert_eq!(gw.default_provider.as_deref(), Some("first"));
    }

    #[test]
    fn test_gateway_set_default() {
        let mut gw = LlmGateway::new();
        gw.register(FakeProvider::new("a"));
        gw.register(FakeProvider::new("b"));
        gw.set_default("b");
        assert_eq!(gw.default_provider.as_deref(), Some("b"));
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
}
