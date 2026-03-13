use clap::Args;

/// Start the RustFlow HTTP API server.
#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Address to bind the server to.
    #[arg(short, long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on.
    #[arg(short, long, default_value = "8080")]
    pub port: u16,
}

pub async fn execute(args: ServeArgs) -> anyhow::Result<()> {
    let addr = format!("{}:{}", args.host, args.port);
    println!("Server starting on http://{addr}");

    let state = rustflow_server::AppState::new();
    let router = rustflow_server::create_router(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Listening on http://{addr}");
    axum::serve(listener, router).await?;
    Ok(())
}
