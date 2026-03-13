use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider '{provider}' not found")]
    ProviderNotFound { provider: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("provider error from '{provider}': {message}")]
    ProviderError { provider: String, message: String },

    #[error("streaming not supported by provider '{provider}'")]
    StreamingNotSupported { provider: String },

    #[error("rate limited by provider '{provider}': retry after {retry_after_secs}s")]
    RateLimited {
        provider: String,
        retry_after_secs: u64,
    },
}

pub type Result<T> = std::result::Result<T, LlmError>;
