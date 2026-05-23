use clap::Args;
use crossterm::style::Stylize;

/// Check system dependencies and configuration.
#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Also run optional checks (Ollama, API keys, running server).
    #[arg(long, conflicts_with = "security")]
    pub full: bool,

    /// Run read-only security checks without printing secret values.
    #[arg(long)]
    pub security: bool,
}

// ── Check result ──────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, PartialEq)]
struct Check {
    label: String,
    status: Status,
    detail: String,
    hint: Option<String>,
}

impl Check {
    fn ok(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: Status::Ok,
            detail: detail.into(),
            hint: None,
        }
    }

    fn warn(label: impl Into<String>, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: Status::Warn,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }

    fn fail(label: impl Into<String>, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            status: Status::Fail,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }
}

// ── Probe helpers ─────────────────────────────────────────────────────────────

fn cmd_version(cmd: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
}

async fn http_get(url: &str, timeout_ms: u64) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| e.to_string())?;

    client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())
}

// ── Checks ────────────────────────────────────────────────────────────────────

fn check_rustc() -> Check {
    match cmd_version("rustc", &["--version"]) {
        Some(v) => Check::ok("Rust toolchain", v),
        None => Check::fail(
            "Rust toolchain",
            "not found",
            "Install via https://rustup.rs",
        ),
    }
}

fn check_cargo() -> Check {
    match cmd_version("cargo", &["--version"]) {
        Some(v) => Check::ok("Cargo", v),
        None => Check::fail("Cargo", "not found", "Install via https://rustup.rs"),
    }
}

fn check_rustflow() -> Check {
    match cmd_version("rustflow", &["--version"]) {
        Some(v) => Check::ok("rustflow binary", v),
        None => Check::warn("rustflow binary", "not found in PATH", "Run: ./install.sh"),
    }
}

fn env_key_check(
    name: &str,
    value: Option<std::ffi::OsString>,
    provider: &str,
    url: &str,
) -> Check {
    let label = format!("{name} env var");
    match value {
        Some(val) if !val.is_empty() => Check::ok(label, "set"),
        _ => Check::warn(
            label,
            "not set",
            format!("Required for {provider} — get a key at {url}"),
        ),
    }
}

fn check_env_key(name: &str, provider: &str, url: &str) -> Check {
    env_key_check(name, std::env::var_os(name), provider, url)
}

async fn check_ollama() -> Check {
    match http_get("http://localhost:11434/api/version", 2000).await {
        Ok(body) => {
            let version = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v["version"].as_str().map(String::from))
                .unwrap_or_else(|| "running".to_string());
            Check::ok("Ollama", format!("v{version} at localhost:11434"))
        }
        Err(_) => Check::warn(
            "Ollama",
            "not reachable on localhost:11434",
            "Install from https://ollama.com — needed for local models",
        ),
    }
}

async fn check_server() -> Check {
    match http_get("http://localhost:18790/health", 1000).await {
        Ok(body) => {
            let version = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v["version"].as_str().map(String::from))
                .unwrap_or_else(|| "ok".to_string());
            Check::ok(
                "RustFlow server",
                format!("running v{version} at localhost:18790"),
            )
        }
        Err(_) => Check::warn(
            "RustFlow server",
            "not running",
            "Start with: rustflow serve",
        ),
    }
}

fn dotenv_check(exists: bool) -> Check {
    if exists {
        Check::ok(".env file", "found in current directory")
    } else {
        Check::warn(
            ".env file",
            "not found in current directory",
            "Copy .env.example to .env and fill in your API keys",
        )
    }
}

fn check_dotenv() -> Check {
    dotenv_check(std::path::Path::new(".env").exists())
}

fn run_store_dir_check(value: Option<std::ffi::OsString>) -> Check {
    match value {
        Some(path) if !path.is_empty() => Check::ok(
            "RUSTFLOW_RUN_STORE_DIR env var",
            "set; using custom run store directory",
        ),
        Some(_) => Check::warn(
            "RUSTFLOW_RUN_STORE_DIR env var",
            "empty; using default .rustflow/runs",
            "Set RUSTFLOW_RUN_STORE_DIR to use a custom run store directory",
        ),
        None => Check::ok(
            "RUSTFLOW_RUN_STORE_DIR env var",
            "not set; using default .rustflow/runs",
        ),
    }
}

fn check_run_store_dir() -> Check {
    run_store_dir_check(std::env::var_os("RUSTFLOW_RUN_STORE_DIR"))
}

fn security_checks() -> Vec<Check> {
    vec![
        check_env_key(
            "OPENAI_API_KEY",
            "OpenAI",
            "https://platform.openai.com/api-keys",
        ),
        check_env_key(
            "ANTHROPIC_API_KEY",
            "Anthropic",
            "https://console.anthropic.com",
        ),
        check_env_key("GLM_API_KEY", "GLM", "https://bigmodel.cn"),
        check_dotenv(),
        check_run_store_dir(),
    ]
}

// ── Render ────────────────────────────────────────────────────────────────────

fn print_check(c: &Check) {
    let (icon, label) = match c.status {
        Status::Ok => (
            format!("{}", "✓".green().bold()),
            c.label.clone().green().to_string(),
        ),
        Status::Warn => (
            format!("{}", "!".yellow().bold()),
            c.label.clone().yellow().to_string(),
        ),
        Status::Fail => (
            format!("{}", "✗".red().bold()),
            c.label.clone().red().to_string(),
        ),
    };
    println!("  [{icon}] {label:<28}  {}", c.detail.as_str().dark_grey());
    if let Some(hint) = &c.hint {
        println!("       {}  {hint}", " ".repeat(28).dark_grey());
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn execute(args: DoctorArgs) -> anyhow::Result<()> {
    println!();
    println!("  {}  RustFlow Doctor", "⚕".bold());
    println!();

    let mut checks: Vec<Check> = if args.security {
        println!(
            "  {}  Running read-only security checks...\n",
            "·".dark_grey()
        );
        security_checks()
    } else {
        vec![
            check_rustc(),
            check_cargo(),
            check_rustflow(),
            check_dotenv(),
        ]
    };

    if args.full {
        println!("  {}  Running full checks...\n", "·".dark_grey());

        // LLM provider env vars
        checks.push(check_env_key(
            "OPENAI_API_KEY",
            "OpenAI",
            "https://platform.openai.com/api-keys",
        ));
        checks.push(check_env_key(
            "ANTHROPIC_API_KEY",
            "Anthropic",
            "https://console.anthropic.com",
        ));
        checks.push(check_env_key("GLM_API_KEY", "GLM", "https://bigmodel.cn"));

        // Network checks
        checks.push(check_ollama().await);
        checks.push(check_server().await);
    }

    let fails = checks.iter().filter(|c| c.status == Status::Fail).count();
    let warns = checks.iter().filter(|c| c.status == Status::Warn).count();

    for c in &checks {
        print_check(c);
    }

    println!();

    if fails == 0 && warns == 0 {
        println!("  {}  All checks passed.", "✓".green().bold());
    } else {
        if fails > 0 {
            println!("  {}  {} check(s) failed.", "✗".red().bold(), fails);
        }
        if warns > 0 {
            println!(
                "  {}  {} check(s) need attention.",
                "!".yellow().bold(),
                warns
            );
        }
        if !args.full && !args.security {
            println!();
            println!(
                "  {}  Run `rustflow doctor --full` for provider/server checks or `rustflow doctor --security` for security checks.",
                "·".dark_grey()
            );
        }
    }

    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_key_check_reports_set_without_secret_material() {
        let check = env_key_check(
            "OPENAI_API_KEY",
            Some("sk-test-secret".into()),
            "OpenAI",
            "https://platform.openai.com/api-keys",
        );

        assert_eq!(check.status, Status::Ok);
        assert_eq!(check.detail, "set");
        assert!(!check.detail.contains("sk-test"));
        assert!(check.hint.is_none());
    }

    #[test]
    fn env_key_check_reports_not_set() {
        let check = env_key_check("GLM_API_KEY", None, "GLM", "https://bigmodel.cn");

        assert_eq!(check.status, Status::Warn);
        assert_eq!(check.detail, "not set");
    }

    #[test]
    fn dotenv_check_reports_presence_and_absence() {
        let present = dotenv_check(true);
        let absent = dotenv_check(false);

        assert_eq!(present.status, Status::Ok);
        assert_eq!(present.detail, "found in current directory");
        assert_eq!(absent.status, Status::Warn);
        assert_eq!(absent.detail, "not found in current directory");
    }

    #[test]
    fn run_store_dir_check_reports_default_and_custom_status() {
        let defaulted = run_store_dir_check(None);
        let custom = run_store_dir_check(Some("/tmp/rustflow-runs".into()));

        assert_eq!(defaulted.status, Status::Ok);
        assert_eq!(defaulted.detail, "not set; using default .rustflow/runs");
        assert_eq!(custom.status, Status::Ok);
        assert_eq!(custom.detail, "set; using custom run store directory");
        assert!(!custom.detail.contains("/tmp/rustflow-runs"));
    }
}
