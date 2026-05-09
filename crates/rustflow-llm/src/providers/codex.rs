use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{debug, instrument};

use crate::error::{LlmError, Result};
use crate::provider::{LlmProvider, ResponseStream};
use crate::types::{LlmRequest, LlmResponse, Role};

const DEFAULT_CODEX_EXECUTABLE: &str = "codex";
const CODEX_EXECUTABLE_ENV: &str = "RUSTFLOW_CODEX_BIN";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// Codex CLI-backed provider.
///
/// This provider never reads or handles Codex authentication material. It invokes
/// the local Codex CLI as the authentication boundary and captures stdout as the
/// completion response.
pub struct CodexProvider {
    executable: PathBuf,
    timeout: Duration,
}

impl CodexProvider {
    /// Create a provider using `RUSTFLOW_CODEX_BIN` or `codex` by default.
    pub fn new() -> Self {
        let executable = std::env::var_os(CODEX_EXECUTABLE_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CODEX_EXECUTABLE));

        Self {
            executable,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Override the Codex executable path. Useful for tests and custom installs.
    pub fn with_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.executable = executable.into();
        self
    }

    /// Override the subprocess timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn render_prompt(request: &LlmRequest) -> String {
        if request.messages.len() == 1 && request.messages[0].role == Role::User {
            return request.messages[0].content.clone();
        }

        request
            .messages
            .iter()
            .map(|message| {
                let role = match message.role {
                    Role::System => "System",
                    Role::User => "User",
                    Role::Assistant => "Assistant",
                };
                format!("{role}:\n{}", message.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn should_pass_model(model: &str) -> bool {
        let model = model.trim();
        !model.is_empty() && model != "default"
    }

    fn response_model(request: &LlmRequest) -> String {
        let model = request.model.trim();
        if model.is_empty() {
            "default".to_string()
        } else {
            request.model.clone()
        }
    }

    fn provider_error(message: impl Into<String>) -> LlmError {
        LlmError::ProviderError {
            provider: "codex".to_string(),
            message: message.into(),
        }
    }
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for CodexProvider {
    fn name(&self) -> &str {
        "codex"
    }

    #[instrument(skip(self, request), fields(model = %request.model))]
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let prompt = Self::render_prompt(request);

        debug!(
            executable = %self.executable.display(),
            model = %request.model,
            "invoking Codex CLI"
        );

        let mut command = Command::new(&self.executable);
        command
            .arg("--ask-for-approval")
            .arg("never")
            .arg("exec")
            .arg("--color")
            .arg("never")
            .arg("--sandbox")
            .arg("read-only")
            .arg("--skip-git-repo-check")
            .arg("--ephemeral");

        if Self::should_pass_model(&request.model) {
            command.arg("--model").arg(request.model.trim());
        }

        command
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Self::provider_error(format!(
                    "Codex CLI not found at '{}'; install Codex CLI or set {CODEX_EXECUTABLE_ENV}",
                    self.executable.display()
                ))
            } else {
                Self::provider_error(format!("failed to start Codex CLI: {error}"))
            }
        })?;

        let output = tokio::time::timeout(self.timeout, async move {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(prompt.as_bytes()).await.map_err(|error| {
                    Self::provider_error(format!(
                        "failed to write prompt to Codex CLI stdin: {error}"
                    ))
                })?;
            }

            child.wait_with_output().await.map_err(|error| {
                Self::provider_error(format!("failed to read Codex CLI output: {error}"))
            })
        })
        .await
        .map_err(|_| {
            Self::provider_error(format!(
                "Codex CLI timed out after {}s",
                self.timeout.as_secs()
            ))
        })??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let details = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            let code = output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string());

            return Err(Self::provider_error(format!(
                "Codex CLI exited with code {code}: {details}"
            )));
        }

        let content = String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string();

        Ok(LlmResponse {
            content,
            model: Self::response_model(request),
            usage: None,
            stop_reason: Some("stop".to_string()),
        })
    }

    async fn stream(&self, _request: &LlmRequest) -> Result<ResponseStream> {
        Err(LlmError::StreamingNotSupported {
            provider: "codex".to_string(),
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::provider::LlmProvider;
    use crate::types::Message;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rustflow-codex-provider-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    fn write_script(body: &str) -> PathBuf {
        let path = unique_path("script");
        fs::write(&path, body).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[tokio::test]
    async fn complete_invokes_configured_codex_executable() {
        let args_path = unique_path("args");
        let stdin_path = unique_path("stdin");
        let script = write_script(&format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\ncat > {}\nprintf 'mock codex response\\n'\n",
            shell_quote(args_path.to_str().unwrap()),
            shell_quote(stdin_path.to_str().unwrap())
        ));

        let provider = CodexProvider::new()
            .with_executable(&script)
            .with_timeout(Duration::from_secs(5));
        let request = LlmRequest::new(
            "gpt-5.5",
            vec![
                Message::system("Use terse answers."),
                Message::user("Say hello."),
            ],
        );

        let response = provider.complete(&request).await.unwrap();

        assert_eq!(response.content, "mock codex response");
        assert_eq!(response.model, "gpt-5.5");
        assert!(response.usage.is_none());

        let args = fs::read_to_string(&args_path).unwrap();
        let args: Vec<_> = args.lines().collect();
        assert_eq!(
            args,
            vec![
                "--ask-for-approval",
                "never",
                "exec",
                "--color",
                "never",
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "--model",
                "gpt-5.5",
                "-"
            ]
        );

        let stdin = fs::read_to_string(&stdin_path).unwrap();
        assert_eq!(stdin, "System:\nUse terse answers.\n\nUser:\nSay hello.");

        let _ = fs::remove_file(script);
        let _ = fs::remove_file(args_path);
        let _ = fs::remove_file(stdin_path);
    }

    #[tokio::test]
    async fn complete_omits_model_flag_for_default_model() {
        let args_path = unique_path("args");
        let script = write_script(&format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\ncat >/dev/null\nprintf 'ok\\n'\n",
            shell_quote(args_path.to_str().unwrap())
        ));

        let provider = CodexProvider::new().with_executable(&script);
        let request = LlmRequest::new("default", vec![Message::user("Hi")]);

        let response = provider.complete(&request).await.unwrap();

        assert_eq!(response.content, "ok");
        assert_eq!(response.model, "default");

        let args = fs::read_to_string(&args_path).unwrap();
        let args: Vec<_> = args.lines().collect();
        assert_eq!(
            args,
            vec![
                "--ask-for-approval",
                "never",
                "exec",
                "--color",
                "never",
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "-"
            ]
        );

        let _ = fs::remove_file(script);
        let _ = fs::remove_file(args_path);
    }

    #[tokio::test]
    async fn complete_maps_subprocess_failure() {
        let script =
            write_script("#!/bin/sh\ncat >/dev/null\necho 'mock codex failure' >&2\nexit 42\n");
        let provider = CodexProvider::new().with_executable(&script);
        let request = LlmRequest::new("default", vec![Message::user("Hi")]);

        let error = provider.complete(&request).await.unwrap_err();

        assert!(matches!(
            error,
            LlmError::ProviderError { ref provider, ref message }
                if provider == "codex"
                    && message.contains("code 42")
                    && message.contains("mock codex failure")
        ));

        let _ = fs::remove_file(script);
    }

    #[tokio::test]
    async fn complete_maps_missing_cli() {
        let provider = CodexProvider::new().with_executable(unique_path("missing"));
        let request = LlmRequest::new("default", vec![Message::user("Hi")]);

        let error = provider.complete(&request).await.unwrap_err();

        assert!(matches!(
            error,
            LlmError::ProviderError { ref provider, ref message }
                if provider == "codex" && message.contains("Codex CLI not found")
        ));
    }

    #[tokio::test]
    async fn complete_maps_timeout() {
        let script = write_script("#!/bin/sh\nsleep 1\n");
        let provider = CodexProvider::new()
            .with_executable(&script)
            .with_timeout(Duration::from_millis(10));
        let request = LlmRequest::new("default", vec![Message::user("Hi")]);

        let error = provider.complete(&request).await.unwrap_err();

        assert!(matches!(
            error,
            LlmError::ProviderError { ref provider, ref message }
                if provider == "codex" && message.contains("timed out")
        ));

        let _ = fs::remove_file(script);
    }

    #[tokio::test]
    async fn stream_is_not_supported() {
        let provider = CodexProvider::new();
        let request = LlmRequest::new("default", vec![Message::user("Hi")]);

        let error = match provider.stream(&request).await {
            Ok(_) => panic!("Codex streaming should not be supported"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            LlmError::StreamingNotSupported { provider } if provider == "codex"
        ));
    }
}
