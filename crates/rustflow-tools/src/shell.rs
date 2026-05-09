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
    /// The executable to run, or a whitespace-separated command string when
    /// `args` is omitted. Direct execution is the default and does not invoke a
    /// shell.
    command: String,
    /// Arguments passed to `command` in direct mode. Supplying this avoids any
    /// command-string splitting and is the preferred form.
    #[serde(default)]
    args: Option<Vec<String>>,
    /// Run `command` through the platform shell (`sh -c` or `cmd /C`).
    ///
    /// Disabled by default in `SecurityPolicy` even when shell execution is
    /// enabled; direct mode should be used unless shell syntax is required.
    #[serde(default)]
    shell: bool,
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

/// Executes a command and returns stdout, stderr, and exit code.
///
/// Commands run in direct mode by default (`Command::new(program).args(args)`),
/// avoiding shell parsing. Set `shell: true` only for trusted workflows that
/// need shell syntax and whose policy explicitly allows shell mode.
///
/// Shell execution is governed by the `SecurityPolicy`:
/// - Disabled by default; must be explicitly enabled
/// - Optional command whitelist restricts which commands can run
/// - Child environments are cleared and rebuilt from policy whitelists
/// - Output is truncated to prevent memory exhaustion
/// - Timeout is clamped to the policy maximum
pub struct ShellTool {
    policy: Arc<SecurityPolicy>,
}

impl ShellTool {
    pub fn new() -> Self {
        Self {
            policy: Arc::new(SecurityPolicy::default()),
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
                    "description": "Executable to run, or a simple whitespace-separated command string when args is omitted."
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Arguments for direct execution. Prefer this over shell syntax."
                },
                "shell": {
                    "type": "boolean",
                    "default": false,
                    "description": "Run command through the platform shell. Requires policy shell.allow_shell_mode."
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

        let command =
            CommandSpec::from_input(&params).map_err(|reason| ToolError::InvalidInput {
                name: "shell".into(),
                reason,
            })?;

        if params.shell {
            self.policy
                .shell
                .validate_shell_mode(&params.command)
                .map_err(|reason| ToolError::SecurityViolation {
                    name: "shell".into(),
                    reason,
                })?;
        } else {
            self.policy
                .shell
                .validate_executable(&command.program)
                .map_err(|reason| ToolError::SecurityViolation {
                    name: "shell".into(),
                    reason,
                })?;
        }

        let cwd = validate_cwd(&self.policy, params.cwd.as_deref())?;

        let mut cmd = tokio::process::Command::new(&command.program);
        cmd.args(&command.args);
        cmd.current_dir(&cwd);

        // Security: prevent inherited environment leakage. Start empty and copy
        // only policy-approved parent keys plus allowed user-provided keys.
        cmd.env_clear();
        for key in &self.policy.shell.inherited_env_keys {
            if self.policy.shell.can_inherit_env_key(key)
                && let Ok(value) = std::env::var(key)
            {
                cmd.env(key, value);
            }
        }
        for (key, val) in &params.env {
            if self.policy.shell.is_env_key_filtered(key) {
                tracing::warn!(key = %key, "filtered dangerous environment variable");
                continue;
            }
            if !self.policy.shell.can_set_env_key(key) {
                return Err(ToolError::SecurityViolation {
                    name: "shell".into(),
                    reason: format!(
                        "environment variable '{key}' is not in the allowed environment whitelist"
                    ),
                });
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

struct CommandSpec {
    program: String,
    args: Vec<String>,
}

impl CommandSpec {
    fn from_input(input: &ShellInput) -> std::result::Result<Self, String> {
        if input.shell {
            if input.args.as_ref().is_some_and(|args| !args.is_empty()) {
                return Err("args cannot be combined with shell mode".into());
            }

            #[cfg(windows)]
            {
                return Ok(Self {
                    program: "cmd".into(),
                    args: vec!["/C".into(), input.command.clone()],
                });
            }

            #[cfg(not(windows))]
            {
                return Ok(Self {
                    program: "sh".into(),
                    args: vec!["-c".into(), input.command.clone()],
                });
            }
        }

        if let Some(args) = &input.args {
            let program = input.command.trim();
            if program.is_empty() {
                return Err("command is empty".into());
            }
            return Ok(Self {
                program: program.to_string(),
                args: args.clone(),
            });
        }

        let mut parts = input.command.split_whitespace();
        let program = parts
            .next()
            .ok_or_else(|| "command is empty".to_string())?
            .to_string();
        Ok(Self {
            program,
            args: parts.map(ToString::to_string).collect(),
        })
    }
}

fn validate_cwd(
    policy: &SecurityPolicy,
    requested_cwd: Option<&str>,
) -> std::result::Result<std::path::PathBuf, ToolError> {
    let cwd_string = match requested_cwd {
        Some(cwd) => cwd.to_string(),
        None => std::env::current_dir()
            .map_err(|e| ToolError::ExecutionFailed {
                name: "shell".into(),
                reason: format!("cannot determine working directory: {e}"),
            })?
            .to_string_lossy()
            .to_string(),
    };

    let validated =
        policy
            .fs
            .validate_path(&cwd_string)
            .map_err(|reason| ToolError::SecurityViolation {
                name: "shell".into(),
                reason,
            })?;

    let metadata = std::fs::metadata(&validated).map_err(|e| ToolError::SecurityViolation {
        name: "shell".into(),
        reason: format!("working directory '{}' cannot be accessed: {e}", cwd_string),
    })?;
    if !metadata.is_dir() {
        return Err(ToolError::SecurityViolation {
            name: "shell".into(),
            reason: format!("working directory '{}' is not a directory", cwd_string),
        });
    }

    Ok(validated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;
    use std::path::PathBuf;

    fn enabled_shell_tool() -> ShellTool {
        ShellTool::with_policy(Arc::new(SecurityPolicy {
            shell: crate::security::ShellPolicy {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        }))
    }

    fn shell_tool_with_policy(shell: crate::security::ShellPolicy) -> ShellTool {
        ShellTool::with_policy(Arc::new(SecurityPolicy {
            shell,
            ..Default::default()
        }))
    }

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
        let tool = enabled_shell_tool();
        let ctx = Context::new();
        let input = json!({"command": "echo hello"});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["stdout"].as_str().unwrap().trim(), "hello");
        assert_eq!(result.inner()["exit_code"], 0);
    }

    #[tokio::test]
    async fn test_shell_with_env() {
        let tool = shell_tool_with_policy(crate::security::ShellPolicy {
            enabled: true,
            allowed_env_keys: vec!["MY_VAR".into()],
            ..Default::default()
        });
        let ctx = Context::new();
        let input = json!({
            "command": "env",
            "env": {"MY_VAR": "test_value"}
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        assert!(
            result.inner()["stdout"]
                .as_str()
                .unwrap()
                .contains("MY_VAR=test_value")
        );
    }

    #[tokio::test]
    async fn test_shell_rejects_unwhitelisted_env() {
        let tool = enabled_shell_tool();
        let ctx = Context::new();
        let input = json!({
            "command": "env",
            "env": {"MY_VAR": "test_value"}
        });
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }

    #[tokio::test]
    async fn test_shell_does_not_inherit_unwhitelisted_env() {
        // SAFETY: this test only asserts that the child process does not inherit
        // the variable; the value is removed before returning.
        unsafe { std::env::set_var("RUSTFLOW_SECRET_ENV_TEST", "leaked") };

        let tool = enabled_shell_tool();
        let ctx = Context::new();
        let result = tool.execute(json!({"command": "env"}), &ctx).await.unwrap();
        assert!(
            !result.inner()["stdout"]
                .as_str()
                .unwrap()
                .contains("RUSTFLOW_SECRET_ENV_TEST=leaked")
        );

        // SAFETY: cleanup for the variable set above.
        unsafe { std::env::remove_var("RUSTFLOW_SECRET_ENV_TEST") };
    }

    #[tokio::test]
    async fn test_shell_nonzero_exit_check() {
        let tool = enabled_shell_tool();
        let ctx = Context::new();
        let input = json!({"command": "false"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn test_shell_nonzero_exit_no_check() {
        let tool = enabled_shell_tool();
        let ctx = Context::new();
        let input = json!({"command": "false", "check": false});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["exit_code"], 1);
    }

    #[tokio::test]
    async fn test_shell_timeout() {
        let tool = enabled_shell_tool();
        let ctx = Context::new();
        let input = json!({"command": "sleep 10", "timeout_secs": 1});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn test_shell_cwd() {
        let policy = SecurityPolicy {
            shell: crate::security::ShellPolicy {
                enabled: true,
                ..Default::default()
            },
            fs: crate::security::FsPolicy {
                allowed_dirs: vec![PathBuf::from("/tmp")],
                allow_symlinks: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = ShellTool::with_policy(Arc::new(policy));
        let ctx = Context::new();
        let input = json!({"command": "pwd", "cwd": "/tmp"});
        let result = tool.execute(input, &ctx).await.unwrap();
        let stdout = result.inner()["stdout"].as_str().unwrap().trim();
        assert!(stdout == "/tmp" || stdout == "/private/tmp");
    }

    #[tokio::test]
    async fn test_shell_disabled_by_policy() {
        let tool = ShellTool::new();
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
                allowed_env_keys: vec!["LD_PRELOAD".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = ShellTool::with_policy(Arc::new(policy));
        let ctx = Context::new();
        // LD_PRELOAD should be filtered out, so the env var won't be set.
        let input = json!({
            "command": "env",
            "env": {"LD_PRELOAD": "/evil/lib.so"}
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        let stdout = result.inner()["stdout"].as_str().unwrap();
        assert!(!stdout.contains("/evil/lib.so"));
    }

    #[tokio::test]
    async fn test_shell_mode_requires_policy_opt_in() {
        let tool = enabled_shell_tool();
        let ctx = Context::new();
        let input = json!({"command": "echo shell", "shell": true});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }

    #[tokio::test]
    async fn test_shell_mode_executes_when_explicitly_allowed() {
        let tool = shell_tool_with_policy(crate::security::ShellPolicy {
            enabled: true,
            allow_shell_mode: true,
            allowed_env_keys: vec!["MY_VAR".into()],
            ..Default::default()
        });
        let ctx = Context::new();
        let input = json!({
            "command": "echo $MY_VAR",
            "shell": true,
            "env": {"MY_VAR": "expanded"}
        });
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(
            result.inner()["stdout"].as_str().unwrap().trim(),
            "expanded"
        );
    }

    #[tokio::test]
    async fn test_direct_shell_interpreter_is_blocked_without_shell_mode_policy() {
        let tool = enabled_shell_tool();
        let ctx = Context::new();
        let input = json!({"command": "sh", "args": ["-c", "echo blocked"]});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }
}
