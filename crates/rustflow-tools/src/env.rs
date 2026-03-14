use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use rustflow_core::context::Context;
use rustflow_core::types::Value;

use crate::error::{Result, ToolError};
use crate::security::SecurityPolicy;
use crate::tool::Tool;

#[derive(Debug, Deserialize)]
struct EnvInput {
    /// Name of the environment variable to read.
    /// If not provided, returns all environment variables (if allowed by policy).
    name: Option<String>,
    /// Default value if the variable is not set.
    default: Option<String>,
}

/// Reads environment variables.
///
/// When `name` is provided, returns the value of that specific variable.
/// When `name` is omitted, returns all environment variables as an object
/// (if allowed by security policy).
///
/// Environment access is governed by the `SecurityPolicy`:
/// - Dumping all variables is disabled by default
/// - Sensitive variable values (matching patterns like `*KEY*`, `*SECRET*`,
///   `*PASSWORD*`, `*TOKEN*`) are automatically redacted
pub struct EnvTool {
    policy: Arc<SecurityPolicy>,
}

impl EnvTool {
    pub fn new() -> Self {
        Self {
            policy: Arc::new(SecurityPolicy::default()),
        }
    }

    pub fn with_policy(policy: Arc<SecurityPolicy>) -> Self {
        Self { policy }
    }
}

impl Default for EnvTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EnvTool {
    fn name(&self) -> &str {
        "env"
    }

    fn description(&self) -> &str {
        "Read environment variables. Returns a single variable by name, or all variables."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the environment variable. Omit to get all variables."
                },
                "default": {
                    "type": "string",
                    "description": "Default value if the variable is not set."
                }
            }
        })
    }

    #[instrument(skip(self, input, _ctx), fields(tool = "env"))]
    async fn execute(&self, input: serde_json::Value, _ctx: &Context) -> Result<Value> {
        let params: EnvInput =
            serde_json::from_value(input).map_err(|e| ToolError::InvalidInput {
                name: "env".into(),
                reason: e.to_string(),
            })?;

        match params.name {
            Some(var_name) => {
                let raw_value = std::env::var(&var_name)
                    .unwrap_or_else(|_| params.default.unwrap_or_default());

                // Security: redact sensitive variable values.
                let value = self.policy.env.maybe_redact(&var_name, raw_value);

                Ok(Value::from(json!({
                    "name": var_name,
                    "value": value,
                })))
            }
            None => {
                // Security: block dump-all if not allowed.
                if !self.policy.env.allow_all {
                    return Err(ToolError::SecurityViolation {
                        name: "env".into(),
                        reason: "reading all environment variables is disabled by security policy; specify a variable name".into(),
                    });
                }

                let vars: serde_json::Map<String, serde_json::Value> = std::env::vars()
                    .map(|(k, v)| {
                        let redacted = self.policy.env.maybe_redact(&k, v);
                        (k, serde_json::Value::String(redacted))
                    })
                    .collect();
                Ok(Value::from(serde_json::Value::Object(vars)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    #[test]
    fn test_env_tool_name() {
        let tool = EnvTool::new();
        assert_eq!(tool.name(), "env");
    }

    #[tokio::test]
    async fn test_env_read_path() {
        let tool = EnvTool::new();
        let ctx = Context::new();
        let input = json!({"name": "PATH"});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["name"], "PATH");
        assert!(!result.inner()["value"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_env_missing_with_default() {
        let tool = EnvTool::new();
        let ctx = Context::new();
        let input = json!({
            "name": "RUSTFLOW_NONEXISTENT_VAR_12345",
            "default": "fallback"
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["value"], "fallback");
    }

    #[tokio::test]
    async fn test_env_all_vars_blocked_by_default() {
        let tool = EnvTool::new();
        let ctx = Context::new();
        let input = json!({});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }

    #[tokio::test]
    async fn test_env_all_vars_allowed_by_policy() {
        let policy = SecurityPolicy {
            env: crate::security::EnvPolicy {
                allow_all: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = EnvTool::with_policy(Arc::new(policy));
        let ctx = Context::new();
        let input = json!({});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(result.inner().is_object());
    }

    #[tokio::test]
    async fn test_env_sensitive_redaction() {
        // Set a test env var that matches a sensitive pattern.
        // SAFETY: single-threaded test context.
        unsafe { std::env::set_var("RUSTFLOW_TEST_API_KEY", "super-secret-value") };

        let tool = EnvTool::new();
        let ctx = Context::new();
        let input = json!({"name": "RUSTFLOW_TEST_API_KEY"});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["value"], "[REDACTED]");

        // SAFETY: single-threaded test context.
        unsafe { std::env::remove_var("RUSTFLOW_TEST_API_KEY") };
    }

    #[tokio::test]
    async fn test_env_non_sensitive_not_redacted() {
        let tool = EnvTool::new();
        let ctx = Context::new();
        let input = json!({"name": "HOME"});
        let result = tool.execute(input, &ctx).await.unwrap();
        // HOME should not be redacted.
        assert_ne!(result.inner()["value"], "[REDACTED]");
    }
}
