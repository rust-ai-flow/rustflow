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
struct ShellInput {
    /// The command to execute.
    command: String,
    /// Working directory. Defaults to current directory.
    cwd: Option<String>,
    /// Timeout in seconds. Defaults to 60.
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
    /// Environment variables to set for the command.
    #[serde(default)]
    env: std::collections::HashMap<String, String>,
    /// If true, return an error when the command exits with a non-zero code.
    /// Defaults to true.
    #[serde(default = "default_check")]
    check: bool,
}

fn default_timeout() -> u64 {
    60
}

fn default_check() -> bool {
    true
}

/// Executes a shell command and returns stdout, stderr, and exit code.
///
/// Commands are run via `sh -c` on Unix. Supports custom working directory,
/// environment variables, and timeout.
///
/// Shell execution is governed by the `SecurityPolicy`:
/// - Disabled by default; must be explicitly enabled
/// - Optional command whitelist restricts which commands can run
/// - Dangerous environment keys (e.g., `LD_PRELOAD`) are filtered
/// - Output is truncated to prevent memory exhaustion
/// - Timeout is clamped to the policy maximum
pub struct ShellTool {
    policy: Arc<SecurityPolicy>,
}

impl ShellTool {
    pub fn new() -> Self {
        Self {
            policy: Arc::new(SecurityPolicy {
                shell: crate::security::ShellPolicy {
                    enabled: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
        }
    }

    pub fn with_policy(policy: Arc<SecurityPolicy>) -> Self {
        Self { policy }
    }
}

impl Default for ShellTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return stdout, stderr, and exit code."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute."
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory for the command."
                },
                "timeout_secs": {
                    "type": "integer",
                    "default": 60,
                    "description": "Command timeout in seconds."
                },
                "env": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Environment variables to set for the command."
                },
                "check": {
                    "type": "boolean",
                    "default": true,
                    "description": "If true, return an error when exit code is non-zero."
                }
            }
        })
    }

    #[instrument(skip(self, input, _ctx), fields(tool = "shell"))]
    async fn execute(&self, input: serde_json::Value, _ctx: &Context) -> Result<Value> {
        let params: ShellInput =
            serde_json::from_value(input).map_err(|e| ToolError::InvalidInput {
                name: "shell".into(),
                reason: e.to_string(),
            })?;

        // Security: validate command against shell policy.
        self.policy
            .shell
            .validate_command(&params.command)
            .map_err(|reason| ToolError::SecurityViolation {
                name: "shell".into(),
                reason,
            })?;

        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(&params.command);

        if let Some(ref cwd) = params.cwd {
            cmd.current_dir(cwd);
        }

        // Security: filter dangerous env keys.
        for (key, val) in &params.env {
            if self.policy.shell.is_env_key_filtered(key) {
                tracing::warn!(key = %key, "filtered dangerous environment variable");
                continue;
            }
            cmd.env(key, val);
        }

        // Security: clamp timeout to policy maximum.
        let timeout_secs = self.policy.shell.clamp_timeout(params.timeout_secs);
        let timeout = std::time::Duration::from_secs(timeout_secs);

        let output = tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| ToolError::ExecutionFailed {
                name: "shell".into(),
                reason: format!(
                    "command timed out after {}s: {}",
                    timeout_secs, params.command
                ),
            })?
            .map_err(|e| ToolError::ExecutionFailed {
                name: "shell".into(),
                reason: format!("failed to execute command: {e}"),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        // Security: truncate output.
        let stdout = self.policy.shell.truncate_output(stdout);
        let stderr = self.policy.shell.truncate_output(stderr);

        if params.check && exit_code != 0 {
            return Err(ToolError::ExecutionFailed {
                name: "shell".into(),
                reason: format!("command exited with code {exit_code}: {stderr}"),
            });
        }

        Ok(Value::from(json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": exit_code,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    #[test]
    fn test_shell_tool_name() {
        let tool = ShellTool::new();
        assert_eq!(tool.name(), "shell");
    }

    #[test]
    fn test_shell_tool_parameters() {
        let tool = ShellTool::new();
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        let required = params["required"].as_array().unwrap();
        assert!(required.contains(&json!("command")));
    }

    #[tokio::test]
    async fn test_shell_echo() {
        let tool = ShellTool::new();
        let ctx = Context::new();
        let input = json!({"command": "echo hello"});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["stdout"].as_str().unwrap().trim(), "hello");
        assert_eq!(result.inner()["exit_code"], 0);
    }

    #[tokio::test]
    async fn test_shell_with_env() {
        let tool = ShellTool::new();
        let ctx = Context::new();
        let input = json!({
            "command": "echo $MY_VAR",
            "env": {"MY_VAR": "test_value"}
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(
            result.inner()["stdout"].as_str().unwrap().trim(),
            "test_value"
        );
    }

    #[tokio::test]
    async fn test_shell_nonzero_exit_check() {
        let tool = ShellTool::new();
        let ctx = Context::new();
        let input = json!({"command": "exit 1"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn test_shell_nonzero_exit_no_check() {
        let tool = ShellTool::new();
        let ctx = Context::new();
        let input = json!({"command": "exit 42", "check": false});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["exit_code"], 42);
    }

    #[tokio::test]
    async fn test_shell_timeout() {
        let tool = ShellTool::new();
        let ctx = Context::new();
        let input = json!({"command": "sleep 10", "timeout_secs": 1});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn test_shell_cwd() {
        let tool = ShellTool::new();
        let ctx = Context::new();
        let input = json!({"command": "pwd", "cwd": "/tmp"});
        let result = tool.execute(input, &ctx).await.unwrap();
        let stdout = result.inner()["stdout"].as_str().unwrap().trim();
        assert!(stdout == "/tmp" || stdout == "/private/tmp");
    }

    #[tokio::test]
    async fn test_shell_disabled_by_policy() {
        let policy = SecurityPolicy::default(); // shell.enabled = false
        let tool = ShellTool::with_policy(Arc::new(policy));
        let ctx = Context::new();
        let input = json!({"command": "echo hello"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }

    #[tokio::test]
    async fn test_shell_command_whitelist() {
        let policy = SecurityPolicy {
            shell: crate::security::ShellPolicy {
                enabled: true,
                allowed_commands: vec!["echo".into(), "cat".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = ShellTool::with_policy(Arc::new(policy));
        let ctx = Context::new();

        // Allowed command.
        let input = json!({"command": "echo allowed"});
        assert!(tool.execute(input, &ctx).await.is_ok());

        // Blocked command.
        let input = json!({"command": "rm -rf /"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }

    #[tokio::test]
    async fn test_shell_filtered_env_keys() {
        let policy = SecurityPolicy {
            shell: crate::security::ShellPolicy {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = ShellTool::with_policy(Arc::new(policy));
        let ctx = Context::new();
        // LD_PRELOAD should be filtered out, so the env var won't be set.
        let input = json!({
            "command": "echo $LD_PRELOAD",
            "env": {"LD_PRELOAD": "/evil/lib.so"}
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        let stdout = result.inner()["stdout"].as_str().unwrap().trim();
        assert!(stdout.is_empty() || !stdout.contains("/evil/lib.so"));
    }
}
