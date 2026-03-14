use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use rustflow_core::context::Context;
use rustflow_core::types::Value;

use crate::error::{Result, ToolError};
use crate::tool::Tool;

#[derive(Debug, Deserialize)]
struct JsonExtractInput {
    /// The JSON data to query (as a string or object).
    data: serde_json::Value,
    /// Dot-separated path to extract, e.g. "users.0.name" or "data.items".
    /// Use numeric indices for arrays: "items.0", "items.1".
    path: String,
    /// Default value to return if the path doesn't exist.
    default: Option<serde_json::Value>,
}

/// Extracts values from JSON data using a dot-path expression.
///
/// Supports nested objects and arrays. Path format: `"key.subkey.0.field"`,
/// where numeric segments are treated as array indices.
///
/// # Examples
///
/// Given `{"users": [{"name": "Alice"}, {"name": "Bob"}]}`:
/// - `"users.0.name"` → `"Alice"`
/// - `"users.1"` → `{"name": "Bob"}`
/// - `"users.999"` with default `"N/A"` → `"N/A"`
pub struct JsonExtractTool;

impl JsonExtractTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonExtractTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Walk a JSON value along a dot-separated path.
fn json_path(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut current = value;

    // If the input is a string, try to parse it as JSON first.
    let parsed;
    if let serde_json::Value::String(s) = current {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
            parsed = v;
            current = &parsed;
        }
    }

    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        // Try array index first.
        if let Ok(idx) = segment.parse::<usize>() {
            current = current.get(idx)?;
        } else {
            current = current.get(segment)?;
        }
    }

    Some(current.clone())
}

#[async_trait]
impl Tool for JsonExtractTool {
    fn name(&self) -> &str {
        "json_extract"
    }

    fn description(&self) -> &str {
        "Extract a value from JSON data using a dot-path expression (e.g. 'users.0.name')."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["data", "path"],
            "properties": {
                "data": {
                    "description": "The JSON data to query. Can be a JSON object/array or a JSON string."
                },
                "path": {
                    "type": "string",
                    "description": "Dot-separated path (e.g. 'users.0.name'). Numeric segments index arrays."
                },
                "default": {
                    "description": "Default value if path not found. If omitted, returns null."
                }
            }
        })
    }

    #[instrument(skip(self, input, _ctx), fields(tool = "json_extract"))]
    async fn execute(&self, input: serde_json::Value, _ctx: &Context) -> Result<Value> {
        let params: JsonExtractInput =
            serde_json::from_value(input).map_err(|e| ToolError::InvalidInput {
                name: "json_extract".into(),
                reason: e.to_string(),
            })?;

        let result =
            json_path(&params.data, &params.path).unwrap_or_else(|| {
                params.default.unwrap_or(serde_json::Value::Null)
            });

        Ok(Value::from(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    #[test]
    fn test_json_extract_tool_name() {
        let tool = JsonExtractTool::new();
        assert_eq!(tool.name(), "json_extract");
    }

    #[test]
    fn test_json_path_simple() {
        let data = json!({"name": "Alice", "age": 30});
        assert_eq!(json_path(&data, "name"), Some(json!("Alice")));
        assert_eq!(json_path(&data, "age"), Some(json!(30)));
    }

    #[test]
    fn test_json_path_nested() {
        let data = json!({"user": {"name": "Bob", "address": {"city": "NYC"}}});
        assert_eq!(json_path(&data, "user.name"), Some(json!("Bob")));
        assert_eq!(json_path(&data, "user.address.city"), Some(json!("NYC")));
    }

    #[test]
    fn test_json_path_array() {
        let data = json!({"items": ["a", "b", "c"]});
        assert_eq!(json_path(&data, "items.0"), Some(json!("a")));
        assert_eq!(json_path(&data, "items.2"), Some(json!("c")));
        assert_eq!(json_path(&data, "items.99"), None);
    }

    #[test]
    fn test_json_path_from_string() {
        let data = json!("{\"key\": \"value\"}");
        assert_eq!(json_path(&data, "key"), Some(json!("value")));
    }

    #[test]
    fn test_json_path_missing() {
        let data = json!({"a": 1});
        assert_eq!(json_path(&data, "b"), None);
        assert_eq!(json_path(&data, "a.b"), None);
    }

    #[tokio::test]
    async fn test_json_extract_execute() {
        let tool = JsonExtractTool::new();
        let ctx = Context::new();
        let input = json!({
            "data": {"users": [{"name": "Alice"}, {"name": "Bob"}]},
            "path": "users.1.name"
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner(), &json!("Bob"));
    }

    #[tokio::test]
    async fn test_json_extract_with_default() {
        let tool = JsonExtractTool::new();
        let ctx = Context::new();
        let input = json!({
            "data": {"a": 1},
            "path": "b.c",
            "default": "fallback"
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner(), &json!("fallback"));
    }

    #[tokio::test]
    async fn test_json_extract_null_default() {
        let tool = JsonExtractTool::new();
        let ctx = Context::new();
        let input = json!({
            "data": {"a": 1},
            "path": "missing"
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(result.inner().is_null());
    }
}
