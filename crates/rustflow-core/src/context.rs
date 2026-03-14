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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_context_is_empty() {
        let ctx = Context::new();
        assert!(ctx.agent_id.is_none());
        assert!(ctx.step_outputs.is_empty());
        assert!(ctx.vars.is_empty());
    }

    #[test]
    fn test_for_agent() {
        let id = AgentId::new("agent-1");
        let ctx = Context::for_agent(id.clone());
        assert_eq!(ctx.agent_id, Some(id));
        assert!(ctx.step_outputs.is_empty());
    }

    #[test]
    fn test_step_output_set_get() {
        let mut ctx = Context::new();
        let sid = StepId::new("fetch");
        let val = Value::from(serde_json::json!("result"));
        ctx.set_step_output(&sid, val.clone());
        assert_eq!(ctx.get_step_output(&sid), Some(&val));
    }

    #[test]
    fn test_step_output_missing() {
        let ctx = Context::new();
        assert!(ctx.get_step_output(&StepId::new("missing")).is_none());
    }

    #[test]
    fn test_step_output_overwrite() {
        let mut ctx = Context::new();
        let sid = StepId::new("s1");
        ctx.set_step_output(&sid, Value::from(serde_json::json!(1)));
        ctx.set_step_output(&sid, Value::from(serde_json::json!(2)));
        assert_eq!(ctx.get_step_output(&sid).unwrap().as_i64(), Some(2));
    }

    #[test]
    fn test_var_set_get() {
        let mut ctx = Context::new();
        let val = Value::from(serde_json::json!("en"));
        ctx.set_var("language", val.clone());
        assert_eq!(ctx.get_var("language"), Some(&val));
    }

    #[test]
    fn test_var_missing() {
        let ctx = Context::new();
        assert!(ctx.get_var("missing").is_none());
    }

    #[test]
    fn test_var_overwrite() {
        let mut ctx = Context::new();
        ctx.set_var("key", Value::from(serde_json::json!("old")));
        ctx.set_var("key", Value::from(serde_json::json!("new")));
        assert_eq!(ctx.get_var("key").unwrap().as_str(), Some("new"));
    }

    #[test]
    fn test_default_context() {
        let ctx = Context::default();
        assert!(ctx.agent_id.is_none());
        assert!(ctx.step_outputs.is_empty());
        assert!(ctx.vars.is_empty());
    }

    #[test]
    fn test_context_serde_roundtrip() {
        let mut ctx = Context::for_agent(AgentId::new("a1"));
        ctx.set_step_output(&StepId::new("s1"), Value::from(serde_json::json!(42)));
        ctx.set_var("lang", Value::from(serde_json::json!("rust")));

        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: Context = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.agent_id, ctx.agent_id);
        assert_eq!(
            deserialized.get_step_output(&StepId::new("s1")),
            ctx.get_step_output(&StepId::new("s1"))
        );
        assert_eq!(deserialized.get_var("lang"), ctx.get_var("lang"));
    }
}
