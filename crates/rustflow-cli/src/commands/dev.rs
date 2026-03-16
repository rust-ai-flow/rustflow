use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use clap::Args;
use crossterm::style::Stylize;
use tokio::sync::mpsc;
use tracing::info;

use rustflow_core::context::Context;
use rustflow_core::workflow::WorkflowDef;
use rustflow_llm::LlmGateway;
use rustflow_llm::providers::anthropic::AnthropicProvider;
use rustflow_llm::providers::glm::GlmProvider;
use rustflow_llm::providers::ollama::OllamaProvider;
use rustflow_llm::providers::openai::OpenAiProvider;
use rustflow_orchestrator::{DefaultStepExecutor, Scheduler, SchedulerEvent};
use rustflow_tools::registry::ToolRegistry;
use rustflow_tools::security::SecurityPolicy;
use rustflow_tools::{
    EnvTool, FileReadTool, FileWriteTool, HttpTool, JsonExtractTool, ShellTool, SleepTool,
};

use super::progress::LiveProgress;

/// Start the development server with file-watching and automatic re-run.
#[derive(Args, Debug)]
pub struct DevArgs {
    /// Workflow YAML file to watch and run.
    pub file: PathBuf,

    /// Override agent input variables (key=value pairs).
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub vars: Vec<String>,

    /// Poll interval in milliseconds.
    #[arg(long, default_value = "500")]
    pub interval_ms: u64,
}

pub async fn execute(args: DevArgs) -> anyhow::Result<()> {
    let file = args
        .file
        .canonicalize()
        .unwrap_or_else(|_| args.file.clone());

    println!();
    println!("  {}  RustFlow Dev", "⟳".cyan().bold());
    println!("  {}  Watching: {}", "·".dark_grey(), file.display().to_string().cyan());
    println!("  {}", "Press Ctrl+C to stop.".dark_grey());
    println!();

    let mut last_mtime = mtime(&file);

    // Run once immediately on startup.
    run_workflow(&file, &args.vars).await;

    // Poll loop.
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(args.interval_ms)) => {
                let current = mtime(&file);
                if current != last_mtime {
                    last_mtime = current;
                    println!();
                    println!("  {}  File changed — reloading...", "⟳".cyan().bold());
                    println!();
                    run_workflow(&file, &args.vars).await;
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!();
                println!("  {}  Dev server stopped.", "■".dark_grey());
                break;
            }
        }
    }

    Ok(())
}

/// Return the last-modified time of a file, or None if it can't be read.
fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Load, build, and execute a workflow, printing live progress.
async fn run_workflow(file: &PathBuf, raw_vars: &[String]) {
    println!("  {}  {}", "▶".cyan(), file.display().to_string().dark_grey());

    // Parse workflow.
    let workflow = match WorkflowDef::from_file(file) {
        Ok(w) => w,
        Err(e) => {
            println!("  {}  Parse error: {e}", "✗".red().bold());
            return;
        }
    };
    let agent = match workflow.into_agent() {
        Ok(a) => a,
        Err(e) => {
            println!("  {}  Validation error: {e}", "✗".red().bold());
            return;
        }
    };

    println!("  {}  {} ({} steps)", "·".dark_grey(), agent.name.clone().bold(), agent.steps.len());
    println!();

    // LLM gateway.
    let mut gateway = LlmGateway::new();
    if std::env::var("OPENAI_API_KEY").is_ok() {
        gateway.register(OpenAiProvider::from_env());
        info!("registered OpenAI provider");
    }
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        gateway.register(AnthropicProvider::from_env());
        info!("registered Anthropic provider");
    }
    if std::env::var("GLM_API_KEY").is_ok() {
        gateway.register(GlmProvider::from_env());
        info!("registered GLM provider");
    }
    gateway.register(OllamaProvider::default());

    // Tool registry.
    let policy = Arc::new(SecurityPolicy {
        shell: rustflow_tools::security::ShellPolicy { enabled: true, ..Default::default() },
        ..Default::default()
    });
    let mut registry = ToolRegistry::new();
    registry.register(HttpTool::new()).ok();
    registry.register(FileReadTool::with_policy(Arc::clone(&policy))).ok();
    registry.register(FileWriteTool::with_policy(Arc::clone(&policy))).ok();
    registry.register(ShellTool::with_policy(Arc::clone(&policy))).ok();
    registry.register(JsonExtractTool::new()).ok();
    registry.register(EnvTool::with_policy(Arc::clone(&policy))).ok();
    registry.register(SleepTool::new()).ok();

    // Context with CLI vars.
    let mut ctx = Context::for_agent(agent.id.clone());
    for var_str in raw_vars {
        if let Some((key, value)) = var_str.split_once('=') {
            ctx.set_var(
                key.to_string(),
                rustflow_core::types::Value::from(serde_json::Value::String(value.to_string())),
            );
        }
    }

    // Live progress display.
    let progress = Arc::new(std::sync::Mutex::new(
        LiveProgress::new(&agent.steps, &agent.name),
    ));
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<SchedulerEvent>();
    let render_done = Arc::new(tokio::sync::Notify::new());

    {
        let mut p = progress.lock().unwrap();
        p.start();
        p.render();
    }

    // Render loop.
    let render_progress = Arc::clone(&progress);
    let render_done_clone = Arc::clone(&render_done);
    let render_handle = tokio::spawn(async move {
        let tick = { render_progress.lock().unwrap().tick_interval() };
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tick) => {
                    while let Ok(ev) = event_rx.try_recv() {
                        render_progress.lock().unwrap().on_event(&ev);
                    }
                    let mut p = render_progress.lock().unwrap();
                    if p.has_running() { p.render(); }
                }
                Some(ev) = event_rx.recv() => {
                    let mut p = render_progress.lock().unwrap();
                    p.on_event(&ev);
                    p.render();
                }
                _ = render_done_clone.notified() => {
                    while let Ok(ev) = event_rx.try_recv() {
                        render_progress.lock().unwrap().on_event(&ev);
                    }
                    break;
                }
            }
        }
    });

    // Execute.
    let executor = Arc::new(DefaultStepExecutor::new(Arc::new(gateway), Arc::new(registry)));
    let scheduler = Scheduler::new(executor);
    let tx = event_tx.clone();
    let result = scheduler
        .run_with_events(&agent.steps, ctx, move |ev| { tx.send(ev).ok(); })
        .await;

    drop(event_tx);
    render_done.notify_one();
    render_handle.await.ok();

    { progress.lock().unwrap().render_final(); }

    match result {
        Ok(_) => println!("  {}  Completed\n", "✓".green().bold()),
        Err(e) => println!("  {}  Failed: {e}\n", "✗".red().bold()),
    }
}
