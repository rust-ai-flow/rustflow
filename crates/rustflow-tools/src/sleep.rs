use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use rustflow_core::context::Context;
use rustflow_core::types::Value;

use crate::error::{Result, ToolError};
use crate::tool::Tool;

#[derive(Debug, Deserialize)]
struct SleepInput {
    /// Duration to sleep in milliseconds.
    ms: u64,
}

/// Pauses execution for a specified duration.
///
/// Useful for rate limiting between API calls, waiting for external
/// processes, or adding deliberate delays in workflows.
pub struct SleepTool;

impl SleepTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SleepTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        "sleep"
    }

    fn description(&self) -> &str {
        "Pause execution for the specified duration in milliseconds."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["ms"],
            "properties": {
                "ms": {
                    "type": "integer",
                    "description": "Duration to sleep in milliseconds."
                }
            }
        })
    }

    #[instrument(skip(self, input, _ctx), fields(tool = "sleep"))]
    async fn execute(&self, input: serde_json::Value, _ctx: &Context) -> Result<Value> {
        let params: SleepInput =
            serde_json::from_value(input).map_err(|e| ToolError::InvalidInput {
                name: "sleep".into(),
                reason: e.to_string(),
            })?;

        let duration = std::time::Duration::from_millis(params.ms);
        tokio::time::sleep(duration).await;

        Ok(Value::from(json!({
            "slept_ms": params.ms,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    #[test]
    fn test_sleep_tool_name() {
        let tool = SleepTool::new();
        assert_eq!(tool.name(), "sleep");
    }

    #[tokio::test]
    async fn test_sleep_short() {
        let tool = SleepTool::new();
        let ctx = Context::new();
        let start = std::time::Instant::now();
        let input = json!({"ms": 50});
        let result = tool.execute(input, &ctx).await.unwrap();
        let elapsed = start.elapsed();
        assert_eq!(result.inner()["slept_ms"], 50);
        assert!(elapsed.as_millis() >= 40); // Allow some tolerance.
    }

    #[tokio::test]
    async fn test_sleep_invalid_input() {
        let tool = SleepTool::new();
        let ctx = Context::new();
        let input = json!({"ms": "not a number"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput { .. }));
    }
}
