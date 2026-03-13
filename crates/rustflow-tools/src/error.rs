use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool '{name}' not found in registry")]
    NotFound { name: String },

    #[error("tool '{name}' already registered")]
    AlreadyRegistered { name: String },

    #[error("invalid input for tool '{name}': {reason}")]
    InvalidInput { name: String, reason: String },

    #[error("tool '{name}' execution failed: {reason}")]
    ExecutionFailed { name: String, reason: String },

    #[error("HTTP error: {0}")]
    Http(String),
}

pub type Result<T> = std::result::Result<T, ToolError>;
