use clap::Args;
use crossterm::style::Stylize;

/// Start the RustFlow HTTP API server.
#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Address to bind the server to.
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on.
    #[arg(short, long, default_value = "18790")]
    pub port: u16,
}

pub async fn execute(args: ServeArgs) -> anyhow::Result<()> {
    let addr = format!("{}:{}", args.host, args.port);
    let url = format!("http://{addr}");

    println!();
    println!("  {}  RustFlow Server", "▶".cyan().bold());
    println!();
    println!("  {}  {}", "HTTP".bold(), url.clone().cyan());
    println!();
    println!("  {}", "Endpoints:".dark_grey());
    println!("  {}  {:<8} /health", "·".dark_grey(), "GET");
    println!("  {}  {:<8} /agents", "·".dark_grey(), "POST");
    println!("  {}  {:<8} /agents", "·".dark_grey(), "GET");
    println!("  {}  {:<8} /agents/:id", "·".dark_grey(), "GET");
    println!("  {}  {:<8} /agents/:id", "·".dark_grey(), "DELETE");
    println!("  {}  {:<8} /agents/:id/run", "·".dark_grey(), "POST");
    println!("  {}  {:<8} /agents/:id/stream", "·".dark_grey(), "WS");
    println!();
    println!("  {}", "Press Ctrl+C to stop.".dark_grey());
    println!();

    let state = rustflow_server::AppState::new();
    let router = rustflow_server::create_router(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    println!();
    println!("  {}  Server stopped.", "■".dark_grey());
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for Ctrl+C");
}
