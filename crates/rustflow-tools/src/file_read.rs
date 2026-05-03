use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tracing::instrument;

use rustflow_core::context::Context;
use rustflow_core::types::Value;

use crate::error::{Result, ToolError};
use crate::security::SecurityPolicy;
use crate::tool::Tool;

#[derive(Debug, Deserialize)]
struct FileReadInput {
    /// Path to the file to read.
    path: String,
    /// Encoding: "utf8" (default) or "base64" (for binary files).
    #[serde(default = "default_encoding")]
    encoding: String,
}

fn default_encoding() -> String {
    "utf8".to_string()
}

/// Reads the contents of a file and returns it as a string.
///
/// Supports UTF-8 text files by default. For binary files, set `encoding`
/// to `"base64"` to get base64-encoded content.
///
/// File access is governed by the `SecurityPolicy` filesystem sandbox:
/// - Paths are validated against allowed directories
/// - Reads are capped by `fs.max_file_size` and fail rather than returning
///   partial content
/// - Symlinks are rejected by default
/// - Sensitive paths (e.g., `.ssh`, `.env`) are blocked
pub struct FileReadTool {
    policy: Arc<SecurityPolicy>,
}

impl FileReadTool {
    pub fn new() -> Self {
        Self {
            policy: Arc::new(SecurityPolicy::default()),
        }
    }

    pub fn with_policy(policy: Arc<SecurityPolicy>) -> Self {
        Self { policy }
    }
}

impl Default for FileReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Returns the file content as a string (UTF-8) or base64-encoded."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read."
                },
                "encoding": {
                    "type": "string",
                    "enum": ["utf8", "base64"],
                    "default": "utf8",
                    "description": "How to encode the file content: utf8 for text, base64 for binary."
                }
            }
        })
    }

    #[instrument(skip(self, input, _ctx), fields(tool = "file_read"))]
    async fn execute(&self, input: serde_json::Value, _ctx: &Context) -> Result<Value> {
        let params: FileReadInput =
            serde_json::from_value(input).map_err(|e| ToolError::InvalidInput {
                name: "file_read".into(),
                reason: e.to_string(),
            })?;

        // Security: validate path against filesystem policy.
        let validated_path = self
            .policy
            .fs
            .validate_path(&params.path)
            .map_err(|reason| ToolError::SecurityViolation {
                name: "file_read".into(),
                reason,
            })?;

        let metadata =
            tokio::fs::metadata(&validated_path)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    name: "file_read".into(),
                    reason: format!("failed to stat '{}': {e}", params.path),
                })?;
        let metadata_len = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        self.policy
            .fs
            .validate_write_size(metadata_len)
            .map_err(|reason| ToolError::SecurityViolation {
                name: "file_read".into(),
                reason: reason.replace("write size", "read size"),
            })?;

        let file = tokio::fs::File::open(&validated_path).await.map_err(|e| {
            ToolError::ExecutionFailed {
                name: "file_read".into(),
                reason: format!("failed to open '{}': {e}", params.path),
            }
        })?;
        let mut bytes = Vec::with_capacity(metadata_len.min(8192));
        file.take(self.policy.fs.max_file_size as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                name: "file_read".into(),
                reason: format!("failed to read '{}': {e}", params.path),
            })?;
        self.policy
            .fs
            .validate_write_size(bytes.len())
            .map_err(|reason| ToolError::SecurityViolation {
                name: "file_read".into(),
                reason: reason.replace("write size", "read size"),
            })?;

        let content = match params.encoding.as_str() {
            "base64" => {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            }
            _ => String::from_utf8(bytes).map_err(|e| ToolError::ExecutionFailed {
                name: "file_read".into(),
                reason: format!("file is not valid UTF-8: {e}"),
            })?,
        };

        Ok(Value::from(json!({
            "path": params.path,
            "content": content,
            "size": content.len(),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;
    use std::path::PathBuf;

    fn tmp_file_read_tool() -> FileReadTool {
        FileReadTool::with_policy(Arc::new(SecurityPolicy {
            fs: crate::security::FsPolicy {
                allowed_dirs: vec![PathBuf::from("/tmp")],
                ..Default::default()
            },
            ..Default::default()
        }))
    }

    #[test]
    fn test_file_read_tool_name() {
        let tool = FileReadTool::new();
        assert_eq!(tool.name(), "file_read");
    }

    #[test]
    fn test_file_read_tool_parameters() {
        let tool = FileReadTool::new();
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["path"].is_object());
    }

    #[tokio::test]
    async fn test_file_read_missing_file() {
        let tool = tmp_file_read_tool();
        let ctx = Context::new();
        let input = json!({"path": "/tmp/rustflow_nonexistent_file_12345"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn test_file_read_success() {
        let tool = tmp_file_read_tool();
        let ctx = Context::new();

        let path = "/tmp/rustflow_test_file_read.txt";
        tokio::fs::write(path, "hello world").await.unwrap();

        let input = json!({"path": path});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["content"], "hello world");

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn test_file_read_base64() {
        let tool = tmp_file_read_tool();
        let ctx = Context::new();

        let path = "/tmp/rustflow_test_file_read_b64.bin";
        tokio::fs::write(path, &[0u8, 1, 2, 255]).await.unwrap();

        let input = json!({"path": path, "encoding": "base64"});
        let result = tool.execute(input, &ctx).await.unwrap();
        assert_eq!(result.inner()["content"], "AAEC/w==");

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn test_file_read_size_limit() {
        let policy = SecurityPolicy {
            fs: crate::security::FsPolicy {
                allowed_dirs: vec![PathBuf::from("/tmp")],
                max_file_size: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = FileReadTool::with_policy(Arc::new(policy));
        let ctx = Context::new();

        let path = "/tmp/rustflow_test_file_read_big.txt";
        tokio::fs::write(path, "too large").await.unwrap();

        let err = tool.execute(json!({"path": path}), &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));

        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn test_file_read_blocked_path() {
        let tool = FileReadTool::new();
        let ctx = Context::new();
        let input = json!({"path": "/home/user/.ssh/id_rsa"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }

    #[tokio::test]
    async fn test_file_read_sandbox_enforcement() {
        let policy = SecurityPolicy {
            fs: crate::security::FsPolicy {
                allowed_dirs: vec![PathBuf::from("/tmp/rustflow_sandbox_test")],
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = FileReadTool::with_policy(Arc::new(policy));
        let ctx = Context::new();
        let input = json!({"path": "/etc/hosts"});
        let err = tool.execute(input, &ctx).await.unwrap_err();
        assert!(matches!(err, ToolError::SecurityViolation { .. }));
    }
}
