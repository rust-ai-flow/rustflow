use std::collections::HashMap;
use std::sync::Arc;

use tracing::{debug, info};

use crate::error::{Result, ToolError};
use crate::tool::Tool;

/// A thread-safe registry of named tools.
///
/// # Example
/// ```rust,ignore
/// let mut registry = ToolRegistry::new();
/// registry.register(HttpTool::new());
/// let tool = registry.get("http").unwrap();
/// ```
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Returns an error if a tool with the same name already exists.
    pub fn register<T: Tool>(&mut self, tool: T) -> Result<()> {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            return Err(ToolError::AlreadyRegistered { name });
        }
        info!(tool = %name, "registering tool");
        self.tools.insert(name, Arc::new(tool));
        Ok(())
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Result<Arc<dyn Tool>> {
        debug!(tool = %name, "looking up tool");
        self.tools
            .get(name)
            .cloned()
            .ok_or_else(|| ToolError::NotFound {
                name: name.to_string(),
            })
    }

    /// Returns true if a tool with the given name is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// List all registered tool names.
    pub fn list(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tools.keys().map(String::as_str).collect();
        names.sort();
        names
    }

    /// Returns the number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Returns true if no tools are registered.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rustflow_core::context::Context;
    use rustflow_core::types::Value;

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
}
