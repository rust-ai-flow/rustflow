use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use rustflow_core::context::Context;
use rustflow_core::types::Value;
use rustflow_tools::error::{Result as ToolResult, ToolError};
use rustflow_tools::tool::Tool;

use crate::instance::PluginInstance;

/// A tool backed by a WASM plugin.
///
/// Each exported tool in a plugin manifest becomes one `PluginTool`.
/// The underlying `PluginInstance` is shared across all tools from the
/// same plugin via an `Arc`, with a `Mutex` serialising access to the store.
pub struct PluginTool {
    tool_name: String,
    description: String,
    parameters_schema: serde_json::Value,
    instance: Arc<PluginInstance>,
}

impl fmt::Debug for PluginTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginTool")
            .field("tool_name", &self.tool_name)
            .field("plugin", &self.instance.manifest.name)
            .finish()
    }
}

impl PluginTool {
    pub fn new(
        tool_name: impl Into<String>,
        description: impl Into<String>,
        parameters_schema: serde_json::Value,
        instance: Arc<PluginInstance>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            description: description.into(),
            parameters_schema,
            instance,
        }
    }
}

#[async_trait]
impl Tool for PluginTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.parameters_schema.clone()
    }

    async fn execute(&self, input: serde_json::Value, _ctx: &Context) -> ToolResult<Value> {
        let instance = Arc::clone(&self.instance);
        let tool_name = self.tool_name.clone();

        // WASM execution is synchronous and CPU-bound — move to a blocking thread.
        let result = tokio::task::spawn_blocking(move || {
            instance.execute_tool_sync(&tool_name, &input)
        })
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            name: self.tool_name.clone(),
            reason: format!("task join error: {e}"),
        })?;

        result.map(Value::from).map_err(ToolError::from)
    }
}
