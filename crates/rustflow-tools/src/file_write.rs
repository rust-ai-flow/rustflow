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
struct FileWriteInput {
    /// Path to write the file to.
    path: String,
    /// Content to write.
    content: String,
    /// If true, append to the file instead of overwriting. Defaults to false.
    #[serde(default)]
    append: bool,
    /// Create parent directories if they don't exist. Defaults to true.
    #[serde(default = "default_mkdir")]
    mkdir: bool,
}

fn default_mkdir() -> bool {
    true
}

/// Writes content to a file, creating it if it doesn't exist.
///
/// Supports overwrite (default) and append modes. Can optionally create
/// parent directories.
///
/// File access is governed by the `SecurityPolicy` filesystem sandbox:
/// - Paths are validated against allowed directories
/// - Write size is checked against the max file size limit
/// - Symlinks are rejected by default
/// - Sensitive paths (e.g., `.ssh`, `.env`) are blocked
pub struct FileWriteTool {
    policy: Arc<SecurityPolicy>,
}

impl FileWriteTool {
    pub fn new() -> Self {
        Self {
            policy: Arc::new(SecurityPolicy::default()),
        }
    }

    pub fn with_policy(policy: Arc<SecurityPolicy>) -> Self {
        Self { policy }
    }
}

impl Default for FileWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write content to a file. Supports overwrite and append modes."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to write the file to."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file."
                },
                "append": {
                    "type": "boolean",
                    "default": false,
                    "description": "If true, append to existing file instead of overwriting."
                },
                "mkdir": {
                    "type": "boolean",
                    "default": true,
                    "description": "Create parent directories if they don't exist."
                }
            }
        })
    }

    #[instrument(skip(self, input, _ctx), fields(tool = "file_write"))]
    async fn execute(&self, input: serde_json::Value, _ctx: &Context) -> Result<Value> {
        let params: FileWriteInput =
            serde_json::from_value(input).map_err(|e| ToolError::InvalidInput {
                name: "file_write".into(),
                reason: e.to_string(),
            })?;

        // Security: validate path against filesystem policy.
        self.policy.fs.validate_path(&params.path).map_err(|reason| {
            ToolError::SecurityViolation {
                name: "file_write".into(),
                reason,
            }
        })?;

        // Security: validate write size.
        self.policy
            .fs
            .validate_write_size(params.content.len())
            .map_err(|reason| ToolError::SecurityViolation {
                name: "file_write".into(),
                reason,
            })?;

        // Create parent directories if requested.
        if params.mkdir {
            if let Some(parent) = std::path::Path::new(&params.path).parent() {
                if !parent.as_os_str().is_empty() {
                    tokio::fs::create_dir_all(parent).await.map_err(|e| {
                        ToolError::ExecutionFailed {
                            name: "file_write".into(),
                            reason: format!("failed to create directories: {e}"),
                        }
                    })?;
                }
            }
        }

        if params.append {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&params.path)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    name: "file_write".into(),
                    reason: format!("failed to open '{}': {e}", params.path),
                })?;
            file.write_all(params.content.as_bytes())
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    name: "file_write".into(),
                    reason: format!("failed to write: {e}"),
                })?;
        } else {
            tokio::fs::write(&params.path, &params.content)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    name: "file_write".into(),
                    reason: format!("failed to write '{}': {e}", params.path),
                })?;
        }

        let bytes_written = params.content.len();
        Ok(Value::from(json!({
            "path": params.path,
            "bytes_written": bytes_written,
            "append": params.append,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;
    use std::path::PathBuf;

    #[test]
    fn test_file_write_tool_name() {
        let tool = FileWriteTool::new();
        assert_eq!(tool.name(), "file_write");
    }

    #[tokio::test]
    async fn test_file_write_and_read_back() {
        let tool = FileWriteTool::new();
        let ctx = Context::new();
        let path = "/tmp/rustflow_test_write.txt";

        let input = json!({"path": path, "content": "test content"});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["bytes_written"], 12);

        let content = tokio::fs::read_to_string(path).await.unwrap();
        assert_eq!(content, "test content");

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn test_file_write_append() {
        let tool = FileWriteTool::new();
        let ctx = Context::new();
        let path = "/tmp/rustflow_test_append.txt";

        let input = json!({"path": path, "content": "hello "});
        tool.execute(input, &ctx).await.unwrap();

        let input = json!({"path": path, "content": "world", "append": true});
        tool.execute(input, &ctx).await.unwrap();

        let content = tokio::fs::read_to_string(path).await.unwrap();
        assert_eq!(content, "hello world");

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn test_file_write_mkdir() {
        let tool = FileWriteTool::new();
        let ctx = Context::new();
        let path = "/tmp/rustflow_test_mkdir_dir/sub/file.txt";

        let input = json!({"path": path, "content": "nested"});
        tool.execute(input, &ctx).await.unwrap();

        let content = tokio::fs::read_to_string(path).await.unwrap();
        assert_eq!(content, "nested");

        tokio::fs::remove_dir_all("/tmp/rustflow_test_mkdir_dir").await.ok();
    }

    #[tokio::test]
    async fn test_file_write_blocked_path() {
        let tool = FileWriteTool::new();
        let ctx = Context::new();
        let input = json!({"path": "/home/user/.ssh/authorized_keys", "content": "hack"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }

    #[tokio::test]
    async fn test_file_write_size_limit() {
        let policy = SecurityPolicy {
            fs: crate::security::FsPolicy {
                max_file_size: 10,
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = FileWriteTool::with_policy(Arc::new(policy));
        let ctx = Context::new();
        let input = json!({"path": "/tmp/rustflow_test_big.txt", "content": "a]".repeat(20)});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }

    #[tokio::test]
    async fn test_file_write_sandbox_enforcement() {
        let policy = SecurityPolicy {
            fs: crate::security::FsPolicy {
                allowed_dirs: vec![PathBuf::from("/tmp/rustflow_sandbox_test")],
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = FileWriteTool::with_policy(Arc::new(policy));
        let ctx = Context::new();
        let input = json!({"path": "/var/log/evil.log", "content": "data"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }
}
