use async_trait::async_trait;

use rustflow_core::context::Context;
use rustflow_core::types::Value;
use rustflow_tools::error::{Result, ToolError};
use rustflow_tools::registry::ToolRegistry;
use rustflow_tools::tool::Tool;

/// A minimal no-op tool for testing the registry.
struct NoopTool {
    tool_name: String,
}

impl NoopTool {
    fn new(name: &str) -> Self {
        Self {
            tool_name: name.to_string(),
        }
    }
}

#[async_trait]
impl Tool for NoopTool {
    fn name(&self) -> &str {
        &self.tool_name
    }
    fn description(&self) -> &str {
        "A no-op tool for testing"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(&self, _input: serde_json::Value, _ctx: &Context) -> Result<Value> {
        Ok(Value::null())
    }
}

#[test]
fn test_registry_new_is_empty() {
    let reg = ToolRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn test_registry_register_and_get() {
    let mut reg = ToolRegistry::new();
    reg.register(NoopTool::new("echo")).unwrap();
    assert!(reg.contains("echo"));
    assert_eq!(reg.len(), 1);

    let tool = reg.get("echo").unwrap();
    assert_eq!(tool.name(), "echo");
}

#[test]
fn test_registry_duplicate_registration() {
    let mut reg = ToolRegistry::new();
    reg.register(NoopTool::new("dup")).unwrap();
    let err = reg.register(NoopTool::new("dup")).unwrap_err();
    assert!(matches!(err, ToolError::AlreadyRegistered { .. }));
}

#[test]
fn test_registry_get_not_found() {
    let reg = ToolRegistry::new();
    let result = reg.get("missing");
    assert!(result.is_err());
    match result.err().unwrap() {
        ToolError::NotFound { name } => assert_eq!(name, "missing"),
        other => panic!("expected NotFound, got: {other:?}"),
    }
}

#[test]
fn test_registry_contains() {
    let mut reg = ToolRegistry::new();
    reg.register(NoopTool::new("x")).unwrap();
    assert!(reg.contains("x"));
    assert!(!reg.contains("y"));
}

#[test]
fn test_registry_list_sorted() {
    let mut reg = ToolRegistry::new();
    reg.register(NoopTool::new("zebra")).unwrap();
    reg.register(NoopTool::new("alpha")).unwrap();
    reg.register(NoopTool::new("middle")).unwrap();
    assert_eq!(reg.list(), vec!["alpha", "middle", "zebra"]);
}

#[test]
fn test_registry_len_and_is_empty() {
    let mut reg = ToolRegistry::new();
    assert!(reg.is_empty());
    reg.register(NoopTool::new("a")).unwrap();
    assert!(!reg.is_empty());
    assert_eq!(reg.len(), 1);
    reg.register(NoopTool::new("b")).unwrap();
    assert_eq!(reg.len(), 2);
}

#[test]
fn test_registry_default() {
    let reg = ToolRegistry::default();
    assert!(reg.is_empty());
}
