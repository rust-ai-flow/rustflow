use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Deref, DerefMut};

/// A transparent wrapper around `serde_json::Value` for use throughout RustFlow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Value(pub serde_json::Value);

impl Value {
    pub fn null() -> Self {
        Self(serde_json::Value::Null)
    }

    pub fn inner(&self) -> &serde_json::Value {
        &self.0
    }

    pub fn into_inner(self) -> serde_json::Value {
        self.0
    }
}

impl Deref for Value {
    type Target = serde_json::Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Value {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<serde_json::Value> for Value {
    fn from(v: serde_json::Value) -> Self {
        Self(v)
    }
}

impl From<Value> for serde_json::Value {
    fn from(v: Value) -> Self {
        v.0
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Newtype wrapper for agent identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl AgentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for AgentId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AgentId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Newtype wrapper for step identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StepId(pub String);

impl StepId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StepId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for StepId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for StepId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Value ────────────────────────────────────────────────────────────

    #[test]
    fn test_value_null() {
        let v = Value::null();
        assert_eq!(*v.inner(), serde_json::Value::Null);
    }

    #[test]
    fn test_value_into_inner() {
        let json = serde_json::json!({"key": "val"});
        let v = Value::from(json.clone());
        assert_eq!(v.into_inner(), json);
    }

    #[test]
    fn test_value_deref() {
        let v = Value::from(serde_json::json!(42));
        assert!(v.is_number());
        assert_eq!(v.as_i64(), Some(42));
    }

    #[test]
    fn test_value_deref_mut() {
        let mut v = Value::from(serde_json::json!({"a": 1}));
        v["b"] = serde_json::json!(2);
        assert_eq!(v["b"], 2);
    }

    #[test]
    fn test_value_from_serde_json() {
        let json = serde_json::json!("hello");
        let v: Value = json.into();
        assert_eq!(v.inner().as_str(), Some("hello"));
    }

    #[test]
    fn test_value_into_serde_json() {
        let v = Value::from(serde_json::json!(true));
        let json: serde_json::Value = v.into();
        assert_eq!(json, serde_json::json!(true));
    }

    #[test]
    fn test_value_display() {
        let v = Value::from(serde_json::json!("test"));
        assert_eq!(format!("{v}"), "\"test\"");
    }

    #[test]
    fn test_value_default() {
        let v = Value::default();
        assert_eq!(*v.inner(), serde_json::Value::Null);
    }

    #[test]
    fn test_value_serde_roundtrip() {
        let v = Value::from(serde_json::json!({"nested": [1, 2, 3]}));
        let serialized = serde_json::to_string(&v).unwrap();
        let deserialized: Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(v, deserialized);
    }

    // ── AgentId ──────────────────────────────────────────────────────────

    #[test]
    fn test_agent_id_new() {
        let id = AgentId::new("agent-1");
        assert_eq!(id.as_str(), "agent-1");
    }

    #[test]
    fn test_agent_id_generate_is_unique() {
        let a = AgentId::generate();
        let b = AgentId::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn test_agent_id_display() {
        let id = AgentId::new("my-agent");
        assert_eq!(format!("{id}"), "my-agent");
    }

    #[test]
    fn test_agent_id_from_string() {
        let id: AgentId = String::from("abc").into();
        assert_eq!(id.as_str(), "abc");
    }

    #[test]
    fn test_agent_id_from_str() {
        let id: AgentId = "xyz".into();
        assert_eq!(id.as_str(), "xyz");
    }

    #[test]
    fn test_agent_id_serde_roundtrip() {
        let id = AgentId::new("test-id");
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: AgentId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn test_agent_id_hash_eq() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(AgentId::new("a"));
        set.insert(AgentId::new("a"));
        assert_eq!(set.len(), 1);
    }

    // ── StepId ───────────────────────────────────────────────────────────

    #[test]
    fn test_step_id_new() {
        let id = StepId::new("step-1");
        assert_eq!(id.as_str(), "step-1");
    }

    #[test]
    fn test_step_id_generate_is_unique() {
        let a = StepId::generate();
        let b = StepId::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn test_step_id_display() {
        let id = StepId::new("fetch");
        assert_eq!(format!("{id}"), "fetch");
    }

    #[test]
    fn test_step_id_from_string() {
        let id: StepId = String::from("s1").into();
        assert_eq!(id.as_str(), "s1");
    }

    #[test]
    fn test_step_id_from_str() {
        let id: StepId = "s2".into();
        assert_eq!(id.as_str(), "s2");
    }

    #[test]
    fn test_step_id_serde_roundtrip() {
        let id = StepId::new("step-x");
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: StepId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }
}
