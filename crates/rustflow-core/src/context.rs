use crate::types::{AgentId, StepId, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Execution context passed through a running agent.
///
/// Holds the current agent's ID, per-step output values, and an arbitrary
/// key-value store for sharing data between steps.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Context {
    /// The ID of the agent this context belongs to.
    pub agent_id: Option<AgentId>,

    /// Outputs produced by each step, keyed by `StepId`.
    pub step_outputs: HashMap<String, Value>,

    /// Arbitrary key-value metadata/shared state.
    pub vars: HashMap<String, Value>,
}

impl Context {
    /// Create an empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a context for a specific agent.
    pub fn for_agent(agent_id: AgentId) -> Self {
        Self {
            agent_id: Some(agent_id),
            ..Default::default()
        }
    }

    /// Record the output of a completed step.
    pub fn set_step_output(&mut self, step_id: &StepId, value: Value) {
        self.step_outputs
            .insert(step_id.as_str().to_string(), value);
    }

    /// Retrieve the output of a previously executed step.
    pub fn get_step_output(&self, step_id: &StepId) -> Option<&Value> {
        self.step_outputs.get(step_id.as_str())
    }

    /// Set a shared variable.
    pub fn set_var(&mut self, key: impl Into<String>, value: Value) {
        self.vars.insert(key.into(), value);
    }

    /// Get a shared variable.
    pub fn get_var(&self, key: &str) -> Option<&Value> {
        self.vars.get(key)
    }
}
