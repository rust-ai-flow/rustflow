use clap::{Parser, Subcommand};
use tracing::info;

mod commands;

/// RustFlow — high-performance AI agent orchestration runtime.
#[derive(Parser, Debug)]
#[command(
    name = "rustflow",
    author,
    version,
    about = "High-performance AI agent orchestration runtime",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose / debug output.
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Output format: text | json.
    #[arg(long, global = true, default_value = "text")]
    output: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run an agent from a YAML workflow file.
    Run(commands::run::RunArgs),

    /// Initialise a new RustFlow project in the current directory.
    Init(commands::init::InitArgs),

    /// Start the RustFlow HTTP API server.
    Serve(commands::serve::ServeArgs),

    /// Check system dependencies and configuration.
    Doctor(commands::doctor::DoctorArgs),

    /// Start the development server with file-watching and hot-reload.
    Dev(commands::dev::DevArgs),

    /// Launch the interactive agent playground.
    Playground(commands::playground::PlaygroundArgs),
}

#[tokio::main]
async fn main() {
    // Load .env file
    if dotenvy::dotenv().is_ok() {
        info!("Loaded .env file");
    }

    let cli = Cli::parse();

    // Initialise tracing.
    // For the `run` command, default to warn level to avoid interference with
    // the live progress display. Use --verbose to see debug/info logs.
    let level = if cli.verbose {
        "debug"
    } else if matches!(cli.command, Commands::Run(_)) {
        "warn"
    } else {
        "info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .init();

    info!(version = env!("CARGO_PKG_VERSION"), "rustflow starting");

    let result = match cli.command {
        Commands::Run(args) => commands::run::execute(args).await,
        Commands::Init(args) => commands::init::execute(args).await,
        Commands::Serve(args) => commands::serve::execute(args).await,
        Commands::Doctor(args) => commands::doctor::execute(args).await,
        Commands::Dev(args) => commands::dev::execute(args).await,
        Commands::Playground(args) => commands::playground::execute(args).await,
    };

    if let Err(e) = result {
        let msg = e.to_string();
        if !msg.is_empty() {
            eprintln!("Error: {msg}");
        }
        std::process::exit(1);
    }
}
