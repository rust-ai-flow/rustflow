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
