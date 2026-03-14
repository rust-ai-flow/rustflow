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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_constructors_and_display() {
        let cases: Vec<(RustFlowError, &str)> = vec![
            (RustFlowError::orchestration("orch fail"), "orchestration error: orch fail"),
            (RustFlowError::llm("llm fail"), "LLM error: llm fail"),
            (RustFlowError::tool("tool fail"), "tool error: tool fail"),
            (RustFlowError::plugin("plugin fail"), "plugin error: plugin fail"),
            (RustFlowError::config("config fail"), "config error: config fail"),
            (RustFlowError::timeout("timed out"), "timeout: timed out"),
            (RustFlowError::circuit_breaker("open"), "circuit breaker open: open"),
        ];

        for (err, expected) in cases {
            assert_eq!(format!("{err}"), expected);
        }
    }

    #[test]
    fn test_error_debug() {
        let err = RustFlowError::config("bad config");
        let debug = format!("{err:?}");
        assert!(debug.contains("Config"));
        assert!(debug.contains("bad config"));
    }

    #[test]
    fn test_result_type_alias() {
        let ok: Result<i32> = Ok(42);
        assert_eq!(ok.unwrap(), 42);

        let err: Result<i32> = Err(RustFlowError::timeout("slow"));
        assert!(err.is_err());
    }
}
