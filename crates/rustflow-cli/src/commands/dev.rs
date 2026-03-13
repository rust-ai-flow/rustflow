use clap::Args;
use std::path::PathBuf;

/// Start the development server with file-watching and hot-reload.
#[derive(Args, Debug)]
pub struct DevArgs {
    /// Directory to watch for changes (default: current directory).
    #[arg(short, long, default_value = ".")]
    pub dir: PathBuf,

    /// Port for the dev server.
    #[arg(short, long, default_value = "8080")]
    pub port: u16,
}

pub async fn execute(args: DevArgs) -> anyhow::Result<()> {
    println!(
        "Dev server watching {} on port {}",
        args.dir.display(),
        args.port
    );
    // TODO: implement file-watching and hot-reload
    println!("not yet implemented");
    Ok(())
}
