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

    #[error("plugin ABI violation: {reason}")]
    AbiViolation { reason: String },

    #[error("WASM trap: {0}")]
    WasmTrap(String),

    #[error("async task join error: {0}")]
    Join(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, PluginError>;

impl From<PluginError> for rustflow_tools::error::ToolError {
    fn from(e: PluginError) -> Self {
        match e {
            PluginError::ExecutionFailed { name, reason } => {
                rustflow_tools::error::ToolError::ExecutionFailed { name, reason }
            }
            other => rustflow_tools::error::ToolError::ExecutionFailed {
                name: "plugin".into(),
                reason: other.to_string(),
            },
        }
    }
}
