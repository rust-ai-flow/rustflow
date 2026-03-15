use anyhow;
use clap::Args;
use crossterm::style::Stylize;
use std::path::PathBuf;
use tokio::process::Command;

/// Launch the web playground (starts the server and opens the browser).
#[derive(Args, Debug)]
pub struct PlaygroundArgs {
    /// Address to bind the server to.
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on.
    #[arg(short, long, default_value = "18790")]
    pub port: u16,

    /// Do not open the browser automatically.
    #[arg(long)]
    pub no_open: bool,
}

/// Returns the playground directory: $RUSTFLOW_PLAYGROUND_DIR if set,
/// otherwise ~/.rustflow/playground.
fn playground_dir() -> anyhow::Result<PathBuf> {
    if let Ok(dir) = std::env::var("RUSTFLOW_PLAYGROUND_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("$HOME is not set"))?;
    Ok(PathBuf::from(home).join(".rustflow").join("playground"))
}

pub async fn execute(args: PlaygroundArgs) -> anyhow::Result<()> {
    let addr = format!("{}:{}", args.host, args.port);
    let url = format!("http://{addr}");
    let frontend_url = "http://localhost:5173/playground/";

    let pg_dir = playground_dir()?;

    if !pg_dir.exists() {
        eprintln!(
            "Error: Playground not found at {}.\n\
             Re-run the installer or set RUSTFLOW_PLAYGROUND_DIR to the playground source directory.",
            pg_dir.display()
        );
        return Err(anyhow::anyhow!("playground directory not found: {}", pg_dir.display()));
    }

    // Check if Node.js is installed
    if !is_command_available("node").await {
        eprintln!("Error: Node.js is not installed. Please install Node.js to run the playground.");
        return Err(anyhow::anyhow!("Node.js not installed"));
    }

    // Check if pnpm is installed
    if !is_command_available("pnpm").await {
        eprintln!("Error: pnpm is not installed. Please install pnpm to run the playground.");
        return Err(anyhow::anyhow!("pnpm not installed"));
    }

    // Install npm dependencies if needed
    if !pg_dir.join("node_modules").exists() {
        eprintln!("Info: Dependencies not installed. Installing dependencies...");
        let status = Command::new("pnpm")
            .args(["install"])
            .current_dir(&pg_dir)
            .status()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run pnpm install: {e}"))?;

        if !status.success() {
            eprintln!(
                "Error: Failed to install dependencies. Try running 'pnpm install' in {}",
                pg_dir.display()
            );
            return Err(anyhow::anyhow!("pnpm install failed"));
        }
        eprintln!("Info: Dependencies installed successfully.");
    }

    // Start frontend development server in the background
    let pg_dir_clone = pg_dir.clone();
    tokio::spawn(async move {
        let mut cmd = Command::new("pnpm")
            .args(["run", "dev"])
            .current_dir(&pg_dir_clone)
            .spawn()
            .expect("Failed to start frontend development server");

        let _ = cmd.wait().await;
    });

    println!();
    println!("  {}  RustFlow Playground", "▶".cyan().bold());
    println!();
    println!("  {}  {}", "HTTP".bold(), url.clone().cyan());
    println!("  {}  {}", "UI  ".bold(), frontend_url.cyan());
    println!();
    println!("  {}", "Press Ctrl+C to stop.".dark_grey());
    println!();

    // Open the browser unless suppressed.
    if !args.no_open {
        // Give the server a moment to start before opening the browser.
        let url = frontend_url.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            open_browser(&url);
        });
    }

    let state = rustflow_server::AppState::new();
    let router = rustflow_server::create_router(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    println!();
    println!("  {}  Playground stopped.", "■".dark_grey());
    Ok(())
}

/// Check if a command is available in the system PATH.
async fn is_command_available(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_ok()
}

/// Open a URL in the system default browser.
fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();

    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", url])
        .spawn();
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for Ctrl+C");
}
