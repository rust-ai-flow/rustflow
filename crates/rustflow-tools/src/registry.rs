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
