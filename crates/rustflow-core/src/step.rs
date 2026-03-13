use crate::retry::RetryPolicy;
use crate::types::StepId;
use serde::{Deserialize, Serialize};

/// The lifecycle state of a step during execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// The step is waiting for its dependencies to complete.
    #[default]
    Pending,
    /// The step is currently executing.
    Running,
    /// The step finished successfully.
    Success,
    /// The step failed and will not be retried.
    Failed,
    /// The step failed and is waiting to be retried.
    Retrying,
}

impl std::fmt::Display for StepState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepState::Pending => write!(f, "pending"),
            StepState::Running => write!(f, "running"),
            StepState::Success => write!(f, "success"),
            StepState::Failed => write!(f, "failed"),
            StepState::Retrying => write!(f, "retrying"),
        }
    }
}

/// Configuration for an LLM-backed step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// The provider to use, e.g. "anthropic", "openai", "ollama".
    pub provider: String,
    /// The model name, e.g. "claude-3-5-sonnet-20241022".
    pub model: String,
    /// The prompt template (may reference `{{step.output}}` variables).
    pub prompt: String,
    /// Sampling temperature (0.0–1.0).
    pub temperature: Option<f32>,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
}

/// Configuration for a tool-backed step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    /// The registered tool name.
    pub tool: String,
    /// Input parameters passed to the tool.
    pub input: serde_json::Value,
}

/// The execution backend for a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Llm(LlmConfig),
    Tool(ToolConfig),
}

/// A single unit of work within an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Unique identifier for this step.
    pub id: StepId,
    /// Human-readable name.
    pub name: String,
    /// What this step does.
    pub kind: StepKind,
    /// IDs of steps that must complete successfully before this step runs.
    pub depends_on: Vec<StepId>,
    /// How to retry this step on failure.
    pub retry_policy: RetryPolicy,
    /// Timeout in milliseconds (None = no timeout).
    pub timeout_ms: Option<u64>,
}

impl Step {
    pub fn new_tool(
        id: impl Into<String>,
        name: impl Into<String>,
        tool: impl Into<String>,
        input: serde_json::Value,
    ) -> Self {
        Self {
            id: StepId::new(id),
            name: name.into(),
            kind: StepKind::Tool(ToolConfig {
                tool: tool.into(),
                input,
            }),
            depends_on: vec![],
            retry_policy: RetryPolicy::None,
            timeout_ms: None,
        }
    }

    pub fn new_llm(
        id: impl Into<String>,
        name: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            id: StepId::new(id),
            name: name.into(),
            kind: StepKind::Llm(LlmConfig {
                provider: provider.into(),
                model: model.into(),
                prompt: prompt.into(),
                temperature: None,
                max_tokens: None,
            }),
            depends_on: vec![],
            retry_policy: RetryPolicy::None,
            timeout_ms: None,
        }
    }

    pub fn with_depends_on(mut self, deps: Vec<StepId>) -> Self {
        self.depends_on = deps;
        self
    }

    pub fn with_retry(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub fn with_timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }
}
