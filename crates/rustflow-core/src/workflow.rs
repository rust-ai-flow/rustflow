use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::agent::Agent;
use crate::error::{Result, RustFlowError};
use crate::retry::RetryPolicy;
use crate::step::{LlmConfig, Step, StepKind, ToolConfig};
use crate::types::StepId;

/// A YAML-friendly workflow definition that deserialises into an `Agent`.
///
/// # YAML Format
///
/// ```yaml
/// name: my-agent
/// description: Fetches and summarises data
/// steps:
///   - id: fetch
///     name: Fetch Data
///     tool:
///       name: http
///       input:
///         url: "https://example.com/data"
///         method: GET
///   - id: summarise
///     name: Summarise
///     llm:
///       provider: openai
///       model: gpt-4
///       prompt: "Summarise the following: {{steps.fetch.output}}"
///     depends_on: [fetch]
///     retry:
///       kind: exponential
///       max_retries: 3
///       initial_interval_ms: 1000
///       multiplier: 2.0
///       max_interval_ms: 30000
///     timeout_ms: 60000
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub steps: Vec<StepDef>,
}

/// A single step definition in the YAML workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDef {
    pub id: String,
    pub name: String,
    /// Tool-backed step.
    #[serde(default)]
    pub tool: Option<ToolStepDef>,
    /// LLM-backed step.
    #[serde(default)]
    pub llm: Option<LlmStepDef>,
    /// Step IDs that must complete before this step.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Retry policy.
    #[serde(default)]
    pub retry: Option<RetryPolicy>,
    /// Timeout in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStepDef {
    pub name: String,
    #[serde(default = "default_null")]
    pub input: serde_json::Value,
}

fn default_null() -> serde_json::Value {
    serde_json::Value::Null
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmStepDef {
    pub provider: String,
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl WorkflowDef {
    /// Parse a workflow definition from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        serde_yaml::from_str(yaml)
            .map_err(|e| RustFlowError::config(format!("invalid workflow YAML: {e}")))
    }

    /// Load a workflow definition from a YAML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| RustFlowError::config(format!("cannot read {}: {e}", path.display())))?;
        Self::from_yaml(&content)
    }

    /// Convert this workflow definition into an `Agent`.
    pub fn into_agent(self) -> Result<Agent> {
        let mut steps = Vec::with_capacity(self.steps.len());

        for def in self.steps {
            let kind = match (def.tool, def.llm) {
                (Some(t), None) => StepKind::Tool(ToolConfig {
                    tool: t.name,
                    input: t.input,
                }),
                (None, Some(l)) => StepKind::Llm(LlmConfig {
                    provider: l.provider,
                    model: l.model,
                    prompt: l.prompt,
                    temperature: l.temperature,
                    max_tokens: l.max_tokens,
                }),
                (Some(_), Some(_)) => {
                    return Err(RustFlowError::config(format!(
                        "step '{}' cannot have both 'tool' and 'llm'",
                        def.id
                    )));
                }
                (None, None) => {
                    return Err(RustFlowError::config(format!(
                        "step '{}' must have either 'tool' or 'llm'",
                        def.id
                    )));
                }
            };

            let step = Step {
                id: StepId::new(&def.id),
                name: def.name,
                kind,
                depends_on: def.depends_on.into_iter().map(StepId::new).collect(),
                retry_policy: def.retry.unwrap_or_default(),
                timeout_ms: def.timeout_ms,
            };
            steps.push(step);
        }

        let mut agent = Agent::new(self.name, steps);
        if let Some(desc) = self.description {
            agent = agent.with_description(desc);
        }

        info!(agent = %agent.name, steps = agent.steps.len(), "workflow loaded");
        Ok(agent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_YAML: &str = r#"
name: test-agent
description: A test workflow
steps:
  - id: fetch
    name: Fetch Data
    tool:
      name: http
      input:
        url: "https://example.com"
        method: GET
  - id: summarise
    name: Summarise
    llm:
      provider: openai
      model: gpt-4
      prompt: "Summarise this"
    depends_on:
      - fetch
    retry:
      kind: fixed
      max_retries: 2
      interval_ms: 1000
    timeout_ms: 30000
"#;

    #[test]
    fn test_parse_workflow() {
        let def = WorkflowDef::from_yaml(SAMPLE_YAML).unwrap();
        assert_eq!(def.name, "test-agent");
        assert_eq!(def.steps.len(), 2);
        assert!(def.steps[0].tool.is_some());
        assert!(def.steps[1].llm.is_some());
        assert_eq!(def.steps[1].depends_on, vec!["fetch"]);
    }

    #[test]
    fn test_workflow_to_agent() {
        let def = WorkflowDef::from_yaml(SAMPLE_YAML).unwrap();
        let agent = def.into_agent().unwrap();
        assert_eq!(agent.name, "test-agent");
        assert_eq!(agent.steps.len(), 2);
        assert_eq!(agent.steps[1].depends_on.len(), 1);
    }

    #[test]
    fn test_rejects_both_tool_and_llm() {
        let yaml = r#"
name: bad
steps:
  - id: x
    name: X
    tool:
      name: http
    llm:
      provider: openai
      model: gpt-4
      prompt: hello
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        assert!(def.into_agent().is_err());
    }

    #[test]
    fn test_rejects_neither_tool_nor_llm() {
        let yaml = r#"
name: bad
steps:
  - id: x
    name: X
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        assert!(def.into_agent().is_err());
    }
}
