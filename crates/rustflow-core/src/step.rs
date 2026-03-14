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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_state_default() {
        assert_eq!(StepState::default(), StepState::Pending);
    }

    #[test]
    fn test_step_state_display() {
        assert_eq!(format!("{}", StepState::Pending), "pending");
        assert_eq!(format!("{}", StepState::Running), "running");
        assert_eq!(format!("{}", StepState::Success), "success");
        assert_eq!(format!("{}", StepState::Failed), "failed");
        assert_eq!(format!("{}", StepState::Retrying), "retrying");
    }

    #[test]
    fn test_step_state_serde_roundtrip() {
        for state in [
            StepState::Pending,
            StepState::Running,
            StepState::Success,
            StepState::Failed,
            StepState::Retrying,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let deserialized: StepState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, deserialized);
        }
    }

    #[test]
    fn test_new_tool_step() {
        let step = Step::new_tool("fetch", "Fetch Data", "http", serde_json::json!({"url": "https://example.com"}));
        assert_eq!(step.id.as_str(), "fetch");
        assert_eq!(step.name, "Fetch Data");
        assert!(matches!(step.kind, StepKind::Tool(_)));
        assert!(step.depends_on.is_empty());
        assert_eq!(step.retry_policy, RetryPolicy::None);
        assert!(step.timeout_ms.is_none());

        if let StepKind::Tool(config) = &step.kind {
            assert_eq!(config.tool, "http");
            assert_eq!(config.input["url"], "https://example.com");
        }
    }

    #[test]
    fn test_new_llm_step() {
        let step = Step::new_llm("summarise", "Summarise", "openai", "gpt-4", "Hello");
        assert_eq!(step.id.as_str(), "summarise");
        assert!(matches!(step.kind, StepKind::Llm(_)));

        if let StepKind::Llm(config) = &step.kind {
            assert_eq!(config.provider, "openai");
            assert_eq!(config.model, "gpt-4");
            assert_eq!(config.prompt, "Hello");
            assert!(config.temperature.is_none());
            assert!(config.max_tokens.is_none());
        }
    }

    #[test]
    fn test_with_depends_on() {
        let step = Step::new_tool("s1", "S1", "http", serde_json::json!(null))
            .with_depends_on(vec![StepId::new("s0")]);
        assert_eq!(step.depends_on.len(), 1);
        assert_eq!(step.depends_on[0].as_str(), "s0");
    }

    #[test]
    fn test_with_retry() {
        let policy = RetryPolicy::Fixed {
            max_retries: 3,
            interval_ms: 1000,
        };
        let step = Step::new_tool("s1", "S1", "http", serde_json::json!(null))
            .with_retry(policy.clone());
        assert_eq!(step.retry_policy, policy);
    }

    #[test]
    fn test_with_timeout_ms() {
        let step = Step::new_tool("s1", "S1", "http", serde_json::json!(null))
            .with_timeout_ms(5000);
        assert_eq!(step.timeout_ms, Some(5000));
    }

    #[test]
    fn test_builder_chaining() {
        let step = Step::new_llm("s1", "S1", "anthropic", "claude", "prompt")
            .with_depends_on(vec![StepId::new("s0")])
            .with_retry(RetryPolicy::Fixed {
                max_retries: 2,
                interval_ms: 500,
            })
            .with_timeout_ms(30000);

        assert_eq!(step.depends_on.len(), 1);
        assert_eq!(step.retry_policy.max_retries(), 2);
        assert_eq!(step.timeout_ms, Some(30000));
    }

    #[test]
    fn test_step_serde_roundtrip() {
        let step = Step::new_tool("fetch", "Fetch", "http", serde_json::json!({"url": "https://example.com"}))
            .with_depends_on(vec![StepId::new("init")])
            .with_retry(RetryPolicy::Exponential {
                max_retries: 3,
                initial_interval_ms: 100,
                multiplier: 2.0,
                max_interval_ms: 5000,
            })
            .with_timeout_ms(10000);

        let json = serde_json::to_string(&step).unwrap();
        let deserialized: Step = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, step.id);
        assert_eq!(deserialized.name, step.name);
        assert_eq!(deserialized.depends_on.len(), 1);
        assert_eq!(deserialized.timeout_ms, Some(10000));
    }
}
