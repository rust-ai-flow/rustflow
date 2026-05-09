use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Security policy configuration for the tool system.
///
/// Controls filesystem access boundaries, shell command execution,
/// and environment variable exposure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityPolicy {
    /// Filesystem security settings.
    #[serde(default)]
    pub fs: FsPolicy,

    /// Shell execution security settings.
    #[serde(default)]
    pub shell: ShellPolicy,

    /// Network target and response-size settings.
    ///
    /// This field was added after the initial public policy shape. It has
    /// compatibility defaults for deserialised configs, but direct struct
    /// literals should use `..Default::default()` so future policy expansion
    /// does not force call-site churn.
    #[serde(default)]
    pub network: NetworkPolicy,

    /// Environment variable security settings.
    #[serde(default)]
    pub env: EnvPolicy,
}

/// Filesystem sandbox policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsPolicy {
    /// Allowed directories for file read/write operations.
    /// If empty, defaults to the current working directory.
    pub allowed_dirs: Vec<PathBuf>,

    /// Maximum file size in bytes for read/write operations (default: 50 MB).
    ///
    /// The original policy exposed one filesystem byte limit for writes. File
    /// reads now reuse the same limit to add a read boundary without changing
    /// the public `FsPolicy` shape.
    #[serde(default = "default_max_file_size")]
    pub max_file_size: usize,

    /// Whether to follow symlinks (default: false — symlinks are rejected).
    #[serde(default)]
    pub allow_symlinks: bool,

    /// Paths that are always blocked (e.g., `/etc/shadow`, `~/.ssh`).
    #[serde(default = "default_blocked_paths")]
    pub blocked_paths: Vec<String>,
}

fn default_max_file_size() -> usize {
    50 * 1024 * 1024 // 50 MB
}

fn default_blocked_paths() -> Vec<String> {
    vec![
        "/etc/shadow".into(),
        "/etc/passwd".into(),
        "/etc/sudoers".into(),
        ".ssh".into(),
        ".gnupg".into(),
        ".aws/credentials".into(),
        ".env".into(),
    ]
}

impl Default for FsPolicy {
    fn default() -> Self {
        Self {
            allowed_dirs: vec![],
            max_file_size: default_max_file_size(),
            allow_symlinks: false,
            blocked_paths: default_blocked_paths(),
        }
    }
}

impl FsPolicy {
    /// Validate a path against the filesystem security policy.
    ///
    /// Returns the canonicalized path if valid, or an error message.
    pub fn validate_path(&self, raw_path: &str) -> Result<PathBuf, String> {
        let path = Path::new(raw_path);

        // Resolve to absolute path (without following symlinks yet).
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("cannot determine working directory: {e}"))?
                .join(path)
        };

        // Check for blocked path patterns.
        let path_str = absolute.to_string_lossy();
        for blocked in &self.blocked_paths {
            if path_str.contains(blocked) {
                return Err(format!(
                    "path '{}' matches blocked pattern '{}'",
                    raw_path, blocked
                ));
            }
        }

        // Check symlinks if not allowed.
        if !self.allow_symlinks && absolute.exists() {
            let metadata = std::fs::symlink_metadata(&absolute)
                .map_err(|e| format!("cannot read metadata for '{}': {e}", raw_path))?;
            if metadata.is_symlink() {
                return Err(format!(
                    "path '{}' is a symlink (symlinks are not allowed by security policy)",
                    raw_path
                ));
            }
        }

        // Canonicalize for existing paths to resolve `..` etc.
        let resolved = if absolute.exists() {
            absolute
                .canonicalize()
                .map_err(|e| format!("cannot canonicalize '{}': {e}", raw_path))?
        } else {
            // For new files, canonicalize the nearest existing ancestor so
            // paths under symlinked directories like /tmp stay comparable to
            // canonicalized allowed directories.
            let mut ancestor = absolute.as_path();
            let mut suffix = PathBuf::new();
            loop {
                if ancestor.exists() {
                    let canonical_ancestor = ancestor.canonicalize().map_err(|e| {
                        format!("cannot canonicalize ancestor of '{}': {e}", raw_path)
                    })?;
                    break canonical_ancestor.join(suffix);
                }

                let name = ancestor
                    .file_name()
                    .ok_or_else(|| format!("path '{}' has no existing ancestor", raw_path))?;
                suffix = if suffix.as_os_str().is_empty() {
                    PathBuf::from(name)
                } else {
                    PathBuf::from(name).join(suffix)
                };
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| format!("path '{}' has no parent directory", raw_path))?;
            }
        };

        // Check allowed directories. An empty allow-list means the current
        // working directory, not unrestricted filesystem access.
        let allowed_dirs = if self.allowed_dirs.is_empty() {
            vec![
                std::env::current_dir()
                    .map_err(|e| format!("cannot determine working directory: {e}"))?,
            ]
        } else {
            self.allowed_dirs.clone()
        };

        let in_allowed = allowed_dirs.iter().any(|dir| {
            let canonical_dir = if dir.exists() {
                dir.canonicalize().unwrap_or_else(|_| dir.clone())
            } else {
                dir.clone()
            };
            resolved.starts_with(&canonical_dir)
        });

        if !in_allowed {
            return Err(format!(
                "path '{}' is outside allowed directories: {:?}",
                raw_path, allowed_dirs
            ));
        }

        Ok(resolved)
    }

    /// Validate write size against the max file size limit.
    pub fn validate_write_size(&self, size: usize) -> Result<(), String> {
        if size > self.max_file_size {
            return Err(format!(
                "write size {} bytes exceeds maximum allowed {} bytes",
                size, self.max_file_size
            ));
        }
        Ok(())
    }
}

/// Shell execution security policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellPolicy {
    /// Whether shell execution is enabled at all (default: false).
    #[serde(default)]
    pub enabled: bool,

    /// Allowed commands (whitelist). If empty and shell is enabled, all commands are allowed.
    /// Commands are matched by executable name in direct mode.
    #[serde(default)]
    pub allowed_commands: Vec<String>,

    /// Whether `shell: true` inputs may run through `sh -c`/`cmd /C`.
    ///
    /// Defaults to false so enabled shell tooling still avoids shell parsing.
    /// Use direct mode with an `args` array for most commands.
    #[serde(default)]
    pub allow_shell_mode: bool,

    /// Parent environment keys to copy into the child process after
    /// `env_clear()`. Defaults to a minimal PATH set so executable lookup works
    /// while avoiding inherited secret leakage.
    #[serde(default = "default_inherited_env_keys")]
    pub inherited_env_keys: Vec<String>,

    /// User-supplied environment keys accepted from the tool input.
    ///
    /// Defaults to empty: command inputs must be explicitly whitelisted by
    /// policy before they can add environment variables.
    #[serde(default)]
    pub allowed_env_keys: Vec<String>,

    /// Environment variable keys that are filtered out before command execution.
    #[serde(default = "default_filtered_env_keys")]
    pub filtered_env_keys: Vec<String>,

    /// Maximum output size in bytes (default: 1 MB).
    #[serde(default = "default_max_output_size")]
    pub max_output_size: usize,

    /// Maximum timeout in seconds (default: 300). Commands cannot set a timeout higher than this.
    #[serde(default = "default_max_timeout")]
    pub max_timeout_secs: u64,
}

#[cfg(not(windows))]
fn default_inherited_env_keys() -> Vec<String> {
    vec!["PATH".into()]
}

#[cfg(windows)]
fn default_inherited_env_keys() -> Vec<String> {
    let mut keys = vec!["PATH".into()];
    keys.extend(["SystemRoot".into(), "PATHEXT".into()]);
    keys
}

fn default_filtered_env_keys() -> Vec<String> {
    vec![
        "LD_PRELOAD".into(),
        "LD_LIBRARY_PATH".into(),
        "DYLD_INSERT_LIBRARIES".into(),
        "DYLD_LIBRARY_PATH".into(),
    ]
}

fn default_max_output_size() -> usize {
    1024 * 1024 // 1 MB
}

fn default_max_timeout() -> u64 {
    300
}

impl Default for ShellPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_commands: vec![],
            allow_shell_mode: false,
            inherited_env_keys: default_inherited_env_keys(),
            allowed_env_keys: vec![],
            filtered_env_keys: default_filtered_env_keys(),
            max_output_size: default_max_output_size(),
            max_timeout_secs: default_max_timeout(),
        }
    }
}

impl ShellPolicy {
    /// Validate an executable against the shell security policy.
    pub fn validate_executable(&self, executable: &str) -> Result<(), String> {
        if !self.enabled {
            return Err("shell execution is disabled by security policy".into());
        }

        if !self.allow_shell_mode && is_shell_executable(executable) {
            return Err(
                "direct execution of shell interpreters is disabled by security policy; use shell mode explicitly"
                    .into(),
            );
        }

        if !self.allowed_commands.is_empty() {
            // Also check the basename (e.g., `/usr/bin/ls` -> `ls`).
            let basename = Path::new(executable)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(executable);

            let allowed = self
                .allowed_commands
                .iter()
                .any(|c| c == executable || c == basename);
            if !allowed {
                return Err(format!(
                    "command '{}' is not in the allowed command list: {:?}",
                    executable, self.allowed_commands
                ));
            }
        }

        Ok(())
    }

    /// Validate a legacy command string against the shell security policy.
    pub fn validate_command(&self, command: &str) -> Result<(), String> {
        let first_word = command.split_whitespace().next().unwrap_or("");
        self.validate_executable(first_word)
    }

    /// Validate explicit shell-mode execution.
    pub fn validate_shell_mode(&self, command: &str) -> Result<(), String> {
        if !self.enabled {
            return Err("shell execution is disabled by security policy".into());
        }
        if !self.allow_shell_mode {
            return Err(
                "shell parsing mode is disabled by security policy; use direct command args or enable shell mode explicitly"
                    .into(),
            );
        }
        if !self.allowed_commands.is_empty() {
            return Err(
                "shell parsing mode cannot be combined safely with an allowed_commands whitelist; use direct command args"
                    .into(),
            );
        }
        if command.trim().is_empty() {
            return Err("command is empty".into());
        }
        Ok(())
    }

    /// Check if an environment key should be filtered.
    pub fn is_env_key_filtered(&self, key: &str) -> bool {
        let upper = key.to_uppercase();
        self.filtered_env_keys
            .iter()
            .any(|k| k.to_uppercase() == upper)
    }

    /// Check whether a parent environment key may be inherited.
    pub fn can_inherit_env_key(&self, key: &str) -> bool {
        env_key_in_list(key, &self.inherited_env_keys) && !self.is_env_key_filtered(key)
    }

    /// Check whether a user-provided environment key may be set.
    pub fn can_set_env_key(&self, key: &str) -> bool {
        env_key_in_list(key, &self.allowed_env_keys) && !self.is_env_key_filtered(key)
    }

    /// Clamp timeout to the max allowed value.
    pub fn clamp_timeout(&self, requested: u64) -> u64 {
        requested.min(self.max_timeout_secs)
    }

    /// Truncate output to max_output_size.
    pub fn truncate_output(&self, output: String) -> String {
        if output.len() > self.max_output_size {
            let end = output
                .char_indices()
                .map(|(idx, _)| idx)
                .take_while(|idx| *idx <= self.max_output_size)
                .last()
                .unwrap_or(0);
            let mut truncated = output[..end].to_string();
            truncated.push_str("\n... [output truncated by security policy]");
            truncated
        } else {
            output
        }
    }
}

fn env_key_in_list(key: &str, list: &[String]) -> bool {
    list.iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(key))
}

fn is_shell_executable(executable: &str) -> bool {
    let basename = Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(executable)
        .to_ascii_lowercase();
    matches!(
        basename.as_str(),
        "sh" | "bash" | "dash" | "zsh" | "fish" | "cmd" | "powershell" | "pwsh"
    )
}

/// Network security policy for tools that contact remote resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicy {
    /// Allow HTTP tools to target localhost, loopback, link-local, unspecified,
    /// or known cloud metadata service addresses.
    ///
    /// Defaults to false to reduce SSRF risk. Set this only for trusted
    /// workflows that intentionally call local services such as Ollama.
    #[serde(default)]
    pub allow_local_targets: bool,

    /// Maximum HTTP response body size in bytes (default: 10 MB).
    #[serde(default = "default_max_http_response_size")]
    pub max_http_response_size: usize,
}

fn default_max_http_response_size() -> usize {
    10 * 1024 * 1024 // 10 MB
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            allow_local_targets: false,
            max_http_response_size: default_max_http_response_size(),
        }
    }
}

impl NetworkPolicy {
    /// Validate a hostname before DNS resolution.
    pub fn validate_host(&self, host: &str) -> Result<(), String> {
        if self.allow_local_targets {
            return Ok(());
        }

        let normalized = host.trim_end_matches('.').to_ascii_lowercase();
        if normalized == "localhost" || normalized.ends_with(".localhost") {
            return Err(format!(
                "network target '{host}' is blocked by security policy; enable allow_local_targets to permit localhost"
            ));
        }

        if let Ok(ip) = normalized.parse::<IpAddr>() {
            self.validate_ip(ip)?;
        }

        Ok(())
    }

    /// Validate a resolved target IP address.
    pub fn validate_ip(&self, ip: IpAddr) -> Result<(), String> {
        if self.allow_local_targets {
            return Ok(());
        }

        if is_blocked_local_or_metadata_ip(ip) {
            return Err(format!(
                "network target '{ip}' is blocked by security policy; enable allow_local_targets to permit local or metadata addresses"
            ));
        }

        Ok(())
    }

    pub fn validate_http_response_size(&self, size: usize) -> Result<(), String> {
        if size > self.max_http_response_size {
            return Err(format!(
                "HTTP response size {} bytes exceeds maximum allowed {} bytes",
                size, self.max_http_response_size
            ));
        }
        Ok(())
    }
}

fn is_blocked_local_or_metadata_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.octets() == [169, 254, 169, 254]
                || ip.octets() == [100, 100, 100, 200]
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
                || ip.to_string().eq_ignore_ascii_case("fd00:ec2::254")
        }
    }
}

/// Environment variable access security policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvPolicy {
    /// Whether reading all environment variables at once is allowed (default: false).
    #[serde(default)]
    pub allow_all: bool,

    /// Patterns for sensitive variable names that will be redacted.
    /// Matched case-insensitively. If the variable name contains any of these
    /// substrings, the value is replaced with `"[REDACTED]"`.
    #[serde(default = "default_sensitive_patterns")]
    pub sensitive_patterns: Vec<String>,
}

fn default_sensitive_patterns() -> Vec<String> {
    vec![
        "KEY".into(),
        "SECRET".into(),
        "PASSWORD".into(),
        "PASSWD".into(),
        "TOKEN".into(),
        "CREDENTIAL".into(),
        "PRIVATE".into(),
    ]
}

impl Default for EnvPolicy {
    fn default() -> Self {
        Self {
            allow_all: false,
            sensitive_patterns: default_sensitive_patterns(),
        }
    }
}

impl EnvPolicy {
    /// Check if a variable name matches a sensitive pattern.
    pub fn is_sensitive(&self, name: &str) -> bool {
        let upper = name.to_uppercase();
        self.sensitive_patterns
            .iter()
            .any(|p| upper.contains(&p.to_uppercase()))
    }

    /// Redact the value if the variable name is sensitive.
    pub fn maybe_redact(&self, name: &str, value: String) -> String {
        if self.is_sensitive(name) {
            "[REDACTED]".to_string()
        } else {
            value
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FsPolicy ──

    #[test]
    fn test_fs_blocked_path() {
        let policy = FsPolicy::default();
        let result = policy.validate_path("/etc/shadow");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("blocked"));
    }

    #[test]
    fn test_fs_blocked_ssh() {
        let policy = FsPolicy::default();
        let result = policy.validate_path("/home/user/.ssh/id_rsa");
        assert!(result.is_err());
    }

    #[test]
    fn test_fs_allowed_dir_enforcement() {
        let policy = FsPolicy {
            allowed_dirs: vec![PathBuf::from("/tmp/rustflow_sandbox")],
            ..Default::default()
        };
        let result = policy.validate_path("/etc/hosts");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("outside allowed"));
    }

    #[test]
    fn test_fs_empty_allowed_dirs_defaults_to_cwd() {
        let policy = FsPolicy::default();
        let cwd_file = std::env::current_dir()
            .unwrap()
            .join("rustflow_security_default_cwd_test.txt");
        let result = policy.validate_path(cwd_file.to_str().unwrap());
        assert!(result.is_ok());

        let outside = if cfg!(target_os = "macos") {
            "/private/tmp/rustflow_security_outside_cwd.txt"
        } else {
            "/tmp/rustflow_security_outside_cwd.txt"
        };
        let result = policy.validate_path(outside);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("outside allowed"));
    }

    #[test]
    fn test_fs_write_size_limit() {
        let policy = FsPolicy {
            max_file_size: 100,
            ..Default::default()
        };
        assert!(policy.validate_write_size(50).is_ok());
        assert!(policy.validate_write_size(200).is_err());
    }

    // ── ShellPolicy ──

    #[test]
    fn test_shell_disabled_by_default() {
        let policy = ShellPolicy::default();
        let result = policy.validate_command("ls");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    fn test_shell_whitelist() {
        let policy = ShellPolicy {
            enabled: true,
            allowed_commands: vec!["ls".into(), "cat".into()],
            ..Default::default()
        };
        assert!(policy.validate_command("ls -la").is_ok());
        assert!(policy.validate_command("cat /tmp/file").is_ok());
        assert!(policy.validate_command("rm -rf /").is_err());
    }

    #[test]
    fn test_shell_enabled_no_whitelist() {
        let policy = ShellPolicy {
            enabled: true,
            allowed_commands: vec![],
            ..Default::default()
        };
        assert!(policy.validate_command("anything").is_ok());
    }

    #[test]
    fn test_shell_filtered_env() {
        let policy = ShellPolicy::default();
        assert!(policy.is_env_key_filtered("LD_PRELOAD"));
        assert!(policy.is_env_key_filtered("DYLD_INSERT_LIBRARIES"));
        assert!(!policy.is_env_key_filtered("HOME"));
    }

    #[test]
    fn test_shell_clamp_timeout() {
        let policy = ShellPolicy {
            max_timeout_secs: 60,
            ..Default::default()
        };
        assert_eq!(policy.clamp_timeout(30), 30);
        assert_eq!(policy.clamp_timeout(120), 60);
    }

    #[test]
    fn test_shell_truncate_output() {
        let policy = ShellPolicy {
            max_output_size: 10,
            ..Default::default()
        };
        let long = "a".repeat(20);
        let truncated = policy.truncate_output(long);
        assert!(truncated.contains("[output truncated"));
        assert!(truncated.starts_with("aaaaaaaaaa"));
    }

    // ── EnvPolicy ──

    #[test]
    fn test_env_sensitive_patterns() {
        let policy = EnvPolicy::default();
        assert!(policy.is_sensitive("OPENAI_API_KEY"));
        assert!(policy.is_sensitive("AWS_SECRET_ACCESS_KEY"));
        assert!(policy.is_sensitive("DB_PASSWORD"));
        assert!(policy.is_sensitive("AUTH_TOKEN"));
        assert!(!policy.is_sensitive("HOME"));
        assert!(!policy.is_sensitive("PATH"));
        assert!(!policy.is_sensitive("LANG"));
    }

    #[test]
    fn test_env_redact() {
        let policy = EnvPolicy::default();
        assert_eq!(
            policy.maybe_redact("API_KEY", "sk-1234".into()),
            "[REDACTED]"
        );
        assert_eq!(
            policy.maybe_redact("HOME", "/home/user".into()),
            "/home/user"
        );
    }

    #[test]
    fn test_env_allow_all_default_false() {
        let policy = EnvPolicy::default();
        assert!(!policy.allow_all);
    }
}
