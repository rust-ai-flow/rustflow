use clap::Args;

/// Launch the interactive agent playground.
#[derive(Args, Debug)]
pub struct PlaygroundArgs {
    /// Port for the playground server.
    #[arg(short, long, default_value = "3000")]
    pub port: u16,

    /// Open in the browser automatically.
    #[arg(long)]
    pub open: bool,
}

pub async fn execute(args: PlaygroundArgs) -> anyhow::Result<()> {
    println!("Playground starting on http://localhost:{}", args.port);
    if args.open {
        println!("Opening browser...");
        // TODO: open system browser
    }
    // TODO: serve playground app
    println!("not yet implemented");
    Ok(())
}
