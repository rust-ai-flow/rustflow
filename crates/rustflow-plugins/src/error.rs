use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin not found at path '{path}'")]
    NotFound { path: String },

    #[error("failed to load plugin '{name}': {reason}")]
    LoadFailed { name: String, reason: String },

    #[error("plugin '{name}' manifest is invalid: {reason}")]
    InvalidManifest { name: String, reason: String },

    #[error("plugin '{name}' execution failed: {reason}")]
    ExecutionFailed { name: String, reason: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, PluginError>;
