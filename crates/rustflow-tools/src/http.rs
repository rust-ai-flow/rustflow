use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, instrument};

use rustflow_core::context::Context;
use rustflow_core::types::Value;

use crate::error::{Result, ToolError};
use crate::tool::Tool;

/// Input parameters for the HTTP tool.
#[derive(Debug, Deserialize)]
struct HttpInput {
    /// The URL to request.
    url: String,
    /// HTTP method (GET, POST, PUT, DELETE, PATCH). Defaults to GET.
    #[serde(default = "default_method")]
    method: String,
    /// Optional request headers.
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    /// Optional JSON body (for POST/PUT/PATCH).
    body: Option<serde_json::Value>,
    /// Timeout in seconds. Defaults to 30.
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_timeout() -> u64 {
    30
}

/// Output produced by a successful HTTP call.
#[derive(Debug, Serialize)]
struct HttpOutput {
    status: u16,
    headers: std::collections::HashMap<String, String>,
    body: serde_json::Value,
}

/// A tool that makes HTTP requests.
///
/// Supports GET, POST, PUT, DELETE, and PATCH with optional JSON bodies and
/// custom headers. The response body is parsed as JSON if possible, otherwise
/// returned as a plain string.
pub struct HttpTool {
    client: reqwest::Client,
}

impl HttpTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("rustflow-tools/0.1")
                .build()
                .expect("failed to build reqwest client"),
        }
    }
}

impl Default for HttpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HttpTool {
    fn name(&self) -> &str {
        "http"
    }

    fn description(&self) -> &str {
        "Make an HTTP request to a URL and return the response status, headers, and body."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to send the request to."
                },
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "DELETE", "PATCH"],
                    "default": "GET",
                    "description": "HTTP method."
                },
                "headers": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Request headers."
                },
                "body": {
                    "description": "JSON body for POST/PUT/PATCH requests."
                },
                "timeout_secs": {
                    "type": "integer",
                    "default": 30,
                    "description": "Request timeout in seconds."
                }
            }
        })
    }

    #[instrument(skip(self, input, _ctx), fields(tool = "http"))]
    async fn execute(&self, input: serde_json::Value, _ctx: &Context) -> Result<Value> {
        let params: HttpInput =
            serde_json::from_value(input).map_err(|e| ToolError::InvalidInput {
                name: "http".to_string(),
                reason: e.to_string(),
            })?;

        let timeout = std::time::Duration::from_secs(params.timeout_secs);
        let method = reqwest::Method::from_bytes(params.method.as_bytes()).map_err(|e| {
            ToolError::InvalidInput {
                name: "http".to_string(),
                reason: format!("invalid HTTP method '{}': {e}", params.method),
            }
        })?;

        debug!(url = %params.url, method = %params.method, "executing HTTP request");

        let mut req = self.client.request(method, &params.url).timeout(timeout);

        for (key, val) in &params.headers {
            req = req.header(key, val);
        }

        if let Some(body) = params.body {
            req = req.json(&body);
        }

        let response = req
            .send()
            .await
            .map_err(|e| ToolError::Http(format!("request to '{}' failed: {e}", params.url)))?;

        let status = response.status().as_u16();
        let resp_headers: std::collections::HashMap<String, String> = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|vs| (k.as_str().to_string(), vs.to_string()))
            })
            .collect();

        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::Http(format!("failed to read response body: {e}")))?;

        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or_else(|_| {
            serde_json::Value::String(String::from_utf8_lossy(&body_bytes).to_string())
        });

        let output = HttpOutput {
            status,
            headers: resp_headers,
            body,
        };

        Ok(Value(serde_json::to_value(output).map_err(|e| {
            ToolError::ExecutionFailed {
                name: "http".to_string(),
                reason: e.to_string(),
            }
        })?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    #[test]
    fn test_http_tool_name() {
        let tool = HttpTool::new();
        assert_eq!(tool.name(), "http");
    }

    #[test]
    fn test_http_tool_description() {
        let tool = HttpTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_http_tool_parameters_schema() {
        let tool = HttpTool::new();
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        let required = params["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("url")));
        assert!(params["properties"]["url"].is_object());
        assert!(params["properties"]["method"].is_object());
        assert!(params["properties"]["headers"].is_object());
        assert!(params["properties"]["timeout_secs"].is_object());
    }

    #[test]
    fn test_http_tool_default() {
        let tool = HttpTool::default();
        assert_eq!(tool.name(), "http");
    }

    #[tokio::test]
    async fn test_http_tool_invalid_input() {
        let tool = HttpTool::new();
        let ctx = Context::new();
        // Missing required "url" field
        let input = serde_json::json!({"method": "GET"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput { .. }));
    }

    #[tokio::test]
    async fn test_http_tool_invalid_method() {
        let tool = HttpTool::new();
        let ctx = Context::new();
        // Method bytes containing a space are invalid per HTTP spec
        let input = serde_json::json!({
            "url": "http://localhost:1",
            "method": "BAD METHOD"
        });
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput { .. }));
    }

    #[tokio::test]
    async fn test_http_tool_connection_refused() {
        let tool = HttpTool::new();
        let ctx = Context::new();
        // Use a port that's almost certainly not listening
        let input = serde_json::json!({
            "url": "http://127.0.0.1:19999",
            "timeout_secs": 1
        });
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::Http(_)));
    }
}
