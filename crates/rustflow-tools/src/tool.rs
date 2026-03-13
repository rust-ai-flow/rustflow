use crate::error::Result;
use async_trait::async_trait;
use rustflow_core::context::Context;
use rustflow_core::types::Value;

/// Every tool in RustFlow implements this trait.
///
/// Tools are invoked by the orchestrator when a `Tool` step is executed.
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    /// The unique registered name of this tool, e.g. `"http"`.
    fn name(&self) -> &str;

    /// A human-readable description of what this tool does.
    fn description(&self) -> &str;

    /// JSON Schema describing the expected input parameters.
    ///
    /// Used by the orchestrator to validate inputs before calling `execute`.
    fn parameters(&self) -> serde_json::Value;

    /// Execute the tool with the given input and agent context.
    ///
    /// Returns a `Value` that will be stored as this step's output.
    async fn execute(&self, input: serde_json::Value, ctx: &Context) -> Result<Value>;
}
