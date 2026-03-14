use crate::step::Step;
use crate::types::AgentId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An agent is a named collection of steps that form a DAG workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Unique identifier.
    pub id: AgentId,
    /// Human-readable name.
    pub name: String,
    /// Optional description of what this agent does.
    pub description: Option<String>,
    /// Ordered list of steps. Dependencies are declared within each `Step`.
    pub steps: Vec<Step>,
    /// Timestamp when this agent was created/registered.
    pub created_at: DateTime<Utc>,
    /// Original YAML definition (optional).
    pub yaml: Option<String>,
}

impl Agent {
    /// Create a new agent with the given name and steps.
    pub fn new(name: impl Into<String>, steps: Vec<Step>) -> Self {
        Self {
            id: AgentId::generate(),
            name: name.into(),
            description: None,
            steps,
            created_at: Utc::now(),
            yaml: None,
        }
    }

    /// Create a new agent with an explicit ID.
    pub fn with_id(id: impl Into<String>, name: impl Into<String>, steps: Vec<Step>) -> Self {
        Self {
            id: AgentId::new(id),
            name: name.into(),
            description: None,
            steps,
            created_at: Utc::now(),
            yaml: None,
        }
    }

    /// Set the original YAML definition.
    pub fn with_yaml(mut self, yaml: impl Into<String>) -> Self {
        self.yaml = Some(yaml.into());
        self
    }

    /// Add a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Find a step by its ID.
    pub fn get_step(&self, step_id: &crate::types::StepId) -> Option<&Step> {
        self.steps.iter().find(|s| &s.id == step_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StepId;

    fn sample_steps() -> Vec<Step> {
        vec![
            Step::new_tool("s1", "Step 1", "http", serde_json::json!({})),
            Step::new_llm("s2", "Step 2", "openai", "gpt-4", "hello"),
        ]
    }

    #[test]
    fn test_agent_new() {
        let agent = Agent::new("test-agent", sample_steps());
        assert_eq!(agent.name, "test-agent");
        assert_eq!(agent.steps.len(), 2);
        assert!(agent.description.is_none());
        assert!(!agent.id.as_str().is_empty());
    }

    #[test]
    fn test_agent_with_id() {
        let agent = Agent::with_id("custom-id", "agent", vec![]);
        assert_eq!(agent.id.as_str(), "custom-id");
        assert_eq!(agent.name, "agent");
    }

    #[test]
    fn test_agent_with_description() {
        let agent = Agent::new("a", vec![]).with_description("does things");
        assert_eq!(agent.description, Some("does things".to_string()));
    }

    #[test]
    fn test_agent_get_step_found() {
        let agent = Agent::new("a", sample_steps());
        let sid = StepId::new("s1");
        let step = agent.get_step(&sid);
        assert!(step.is_some());
        assert_eq!(step.unwrap().name, "Step 1");
    }

    #[test]
    fn test_agent_get_step_not_found() {
        let agent = Agent::new("a", sample_steps());
        let sid = StepId::new("missing");
        assert!(agent.get_step(&sid).is_none());
    }

    #[test]
    fn test_agent_new_generates_unique_ids() {
        let a1 = Agent::new("a", vec![]);
        let a2 = Agent::new("a", vec![]);
        assert_ne!(a1.id, a2.id);
    }

    #[test]
    fn test_agent_serde_roundtrip() {
        let agent = Agent::with_id("id-1", "agent-1", sample_steps()).with_description("test");
        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: Agent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, agent.id);
        assert_eq!(deserialized.name, agent.name);
        assert_eq!(deserialized.description, agent.description);
        assert_eq!(deserialized.steps.len(), 2);
    }
}
