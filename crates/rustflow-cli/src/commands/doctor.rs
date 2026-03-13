use clap::Args;

/// Check system dependencies and configuration.
#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Also check optional dependencies.
    #[arg(long)]
    pub full: bool,
}

struct Check {
    name: &'static str,
    ok: bool,
    message: String,
}

impl Check {
    fn new(name: &'static str, cmd: &str) -> Self {
        let found = cmd_exists(cmd);
        let message = if found {
            if cmd == "rustc" {
                rust_version()
            } else {
                "found".to_string()
            }
        } else {
            match cmd {
                "rustc" => "not found — install via https://rustup.rs".to_string(),
                _ => format!("'{cmd}' not found in PATH"),
            }
        };
        Self {
            name,
            ok: found,
            message,
        }
    }
}

pub async fn execute(args: DoctorArgs) -> anyhow::Result<()> {
    println!("RustFlow Doctor — system check\n");

    let checks: Vec<Check> = vec![
        Check::new("Rust toolchain", "rustc"),
        Check::new("Cargo", "cargo"),
    ];

    let mut all_ok = true;
    for check in &checks {
        let icon = if check.ok { "✓" } else { "✗" };
        println!("  [{icon}] {} — {}", check.name, check.message);
        if !check.ok {
            all_ok = false;
        }
    }

    if args.full {
        println!("\n  [?] Docker — not yet implemented");
        println!("  [?] Ollama  — not yet implemented");
    }

    println!();
    if all_ok {
        println!("All checks passed.");
    } else {
        println!("Some checks failed. Please fix the issues above.");
    }

    Ok(())
}

fn cmd_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn rust_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
