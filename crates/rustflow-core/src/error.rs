use thiserror::Error;

/// The top-level error type for RustFlow.
#[derive(Debug, Error)]
pub enum RustFlowError {
    /// Errors arising from agent/step orchestration logic.
    #[error("orchestration error: {message}")]
    Orchestration { message: String },

    /// Errors returned by an LLM provider.
    #[error("LLM error: {message}")]
    Llm { message: String },

    /// Errors arising from tool execution.
    #[error("tool error: {message}")]
    Tool { message: String },

    /// Errors arising from WASM plugin loading or execution.
    #[error("plugin error: {message}")]
    Plugin { message: String },

    /// Errors in configuration parsing or validation.
    #[error("config error: {message}")]
    Config { message: String },

    /// A step or agent exceeded its configured timeout.
    #[error("timeout: {message}")]
    Timeout { message: String },

    /// Circuit breaker opened due to too many failures.
    #[error("circuit breaker open: {message}")]
    CircuitBreaker { message: String },
}

impl RustFlowError {
    pub fn orchestration(msg: impl Into<String>) -> Self {
        Self::Orchestration {
            message: msg.into(),
        }
    }

    pub fn llm(msg: impl Into<String>) -> Self {
        Self::Llm {
            message: msg.into(),
        }
    }

    pub fn tool(msg: impl Into<String>) -> Self {
        Self::Tool {
            message: msg.into(),
        }
    }

    pub fn plugin(msg: impl Into<String>) -> Self {
        Self::Plugin {
            message: msg.into(),
        }
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config {
            message: msg.into(),
        }
    }

    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout {
            message: msg.into(),
        }
    }

    pub fn circuit_breaker(msg: impl Into<String>) -> Self {
        Self::CircuitBreaker {
            message: msg.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, RustFlowError>;
