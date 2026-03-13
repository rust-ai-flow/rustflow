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
        }
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
