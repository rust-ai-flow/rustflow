use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, instrument};

use rustflow_core::context::Context;
use rustflow_core::types::Value;

use crate::error::{Result, ToolError};
use crate::security::SecurityPolicy;
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
    policy: Arc<SecurityPolicy>,
}

impl HttpTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("rustflow-tools/0.1")
                // Redirect targets must be validated before use. Keeping
                // redirects explicit avoids SSRF bypasses via 30x responses.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("failed to build reqwest client"),
            policy: Arc::new(SecurityPolicy::default()),
        }
    }

    pub fn with_policy(policy: Arc<SecurityPolicy>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("rustflow-tools/0.1")
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("failed to build reqwest client"),
            policy,
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

        let url = reqwest::Url::parse(&params.url).map_err(|e| ToolError::InvalidInput {
            name: "http".to_string(),
            reason: format!("invalid URL '{}': {e}", params.url),
        })?;
        validate_http_target(&self.policy, &url).await?;

        debug!(url = %params.url, method = %params.method, "executing HTTP request");

        let mut req = self.client.request(method, url.clone()).timeout(timeout);

        for (key, val) in &params.headers {
            req = req.header(key, val);
        }

        if let Some(body) = params.body {
            req = req.json(&body);
        }

        let mut response = req
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

        if let Some(len) = response.content_length() {
            let len = usize::try_from(len).unwrap_or(usize::MAX);
            self.policy
                .network
                .validate_http_response_size(len)
                .map_err(|reason| ToolError::SecurityViolation {
                    name: "http".into(),
                    reason,
                })?;
        }

        let body_bytes =
            read_limited_body(&mut response, self.policy.network.max_http_response_size).await?;

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

async fn validate_http_target(policy: &SecurityPolicy, url: &reqwest::Url) -> Result<()> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(ToolError::InvalidInput {
                name: "http".into(),
                reason: format!("unsupported URL scheme '{scheme}'"),
            });
        }
    }

    let host = url.host_str().ok_or_else(|| ToolError::InvalidInput {
        name: "http".into(),
        reason: "URL is missing a host".into(),
    })?;
    policy
        .network
        .validate_host(host)
        .map_err(|reason| ToolError::SecurityViolation {
            name: "http".into(),
            reason,
        })?;

    if !policy.network.allow_local_targets {
        if host.parse::<std::net::IpAddr>().is_err() {
            let port = url
                .port_or_known_default()
                .ok_or_else(|| ToolError::InvalidInput {
                    name: "http".into(),
                    reason: format!(
                        "URL '{}' is missing a port for scheme '{}'",
                        url,
                        url.scheme()
                    ),
                })?;
            let addresses = tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| ToolError::Http(format!("failed to resolve '{host}': {e}")))?;

            for address in addresses {
                policy.network.validate_ip(address.ip()).map_err(|reason| {
                    ToolError::SecurityViolation {
                        name: "http".into(),
                        reason,
                    }
                })?;
            }
        }
    }

    Ok(())
}

async fn read_limited_body(response: &mut reqwest::Response, max_size: usize) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| ToolError::Http(format!("failed to read response body: {e}")))?
    {
        let next_len =
            body.len()
                .checked_add(chunk.len())
                .ok_or_else(|| ToolError::SecurityViolation {
                    name: "http".into(),
                    reason: "HTTP response size overflowed local limits".into(),
                })?;
        if next_len > max_size {
            return Err(ToolError::SecurityViolation {
                name: "http".into(),
                reason: format!(
                    "HTTP response size exceeds maximum allowed {} bytes",
                    max_size
                ),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::NetworkPolicy;
    use crate::tool::Tool;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn serve_once(body: &str) -> Option<String> {
        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(e) => panic!("failed to bind test HTTP server: {e}"),
        };
        let addr = listener.local_addr().unwrap();
        let body = body.to_string();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request_buf = [0_u8; 1024];
            let _ = socket.read(&mut request_buf).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
        Some(format!("http://{addr}"))
    }

    fn local_http_tool(max_http_response_size: usize) -> HttpTool {
        HttpTool::with_policy(Arc::new(SecurityPolicy {
            network: NetworkPolicy {
                allow_local_targets: true,
                max_http_response_size,
            },
            ..Default::default()
        }))
    }

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
        let input = serde_json::json!({
            "url": "http://127.0.0.1:19999",
            "timeout_secs": 1
        });
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }

    #[tokio::test]
    async fn test_http_tool_local_target_allowed_by_policy() {
        let tool = local_http_tool(1024);
        let ctx = Context::new();
        let input = serde_json::json!({
            "url": "http://127.0.0.1:19999",
            "timeout_secs": 1
        });
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::Http(_)));
    }

    #[tokio::test]
    async fn test_http_tool_response_size_limit() {
        let tool = local_http_tool(5);
        let ctx = Context::new();
        let Some(url) = serve_once("too large").await else {
            return;
        };
        let err = tool
            .execute(serde_json::json!({"url": url, "timeout_secs": 1}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }
}
