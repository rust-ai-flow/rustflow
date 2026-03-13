use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("cycle detected in DAG involving step: {step_id}")]
    CycleDetected { step_id: String },

    #[error("unknown dependency '{dependency}' referenced by step '{step_id}'")]
    UnknownDependency { step_id: String, dependency: String },

    #[error("duplicate step id: {step_id}")]
    DuplicateStepId { step_id: String },

    #[error("step '{step_id}' failed: {reason}")]
    StepFailed { step_id: String, reason: String },

    #[error("step '{step_id}' timed out after {timeout_ms}ms")]
    StepTimeout { step_id: String, timeout_ms: u64 },

    #[error("core error: {0}")]
    Core(#[from] rustflow_core::RustFlowError),
}

pub type Result<T> = std::result::Result<T, OrchestratorError>;
