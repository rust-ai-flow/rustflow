use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use crossterm::style::Stylize;
use tokio::sync::mpsc;
use tracing::info;

use rustflow_core::context::Context;
use rustflow_core::types::Value;
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

/// Run an agent from a YAML workflow file.
#[derive(Args, Debug)]
pub struct RunArgs {
    /// Path to the workflow YAML file.
    pub file: PathBuf,

    /// Override agent input variables (key=value pairs).
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub vars: Vec<String>,

    /// Watch for file changes and re-run automatically.
    #[arg(short, long)]
    pub watch: bool,
}

pub async fn execute(args: RunArgs) -> anyhow::Result<()> {
    // --watch: delegate to the dev watcher.
    if args.watch {
        return super::dev::execute(super::dev::DevArgs {
            file: args.file,
            vars: args.vars,
            interval_ms: 500,
        })
        .await;
    }

    println!(
        "{}  Loading workflow: {}",
        "▶".cyan(),
        args.file.display()
    );

    // 1. Parse the workflow YAML.
    let workflow = WorkflowDef::from_file(&args.file).map_err(|e| anyhow::anyhow!("{e}"))?;
    let agent = workflow.into_agent().map_err(|e| anyhow::anyhow!("{e}"))?;

    println!(
        "   Agent: {} ({} steps)",
        agent.name.clone().bold(),
        agent.steps.len()
    );
    println!();

    // 2. Set up the LLM gateway with available providers from env.
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
    // Ollama is always available (local, no API key needed).
    gateway.register(OllamaProvider::default());
    info!("registered Ollama provider");

    // 3. Set up security policy and tool registry.
    let policy = Arc::new(SecurityPolicy {
        shell: rustflow_tools::security::ShellPolicy {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    });

    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(HttpTool::new()).ok();
    tool_registry
        .register(FileReadTool::with_policy(Arc::clone(&policy)))
        .ok();
    tool_registry
        .register(FileWriteTool::with_policy(Arc::clone(&policy)))
        .ok();
    tool_registry
        .register(ShellTool::with_policy(Arc::clone(&policy)))
        .ok();
    tool_registry.register(JsonExtractTool::new()).ok();
    tool_registry
        .register(EnvTool::with_policy(Arc::clone(&policy)))
        .ok();
    tool_registry.register(SleepTool::new()).ok();

    // 4. Build execution context with CLI variables.
    let mut ctx = Context::for_agent(agent.id.clone());
    for var_str in &args.vars {
        if let Some((key, value)) = var_str.split_once('=') {
            ctx.set_var(
                key.to_string(),
                Value::from(serde_json::Value::String(value.to_string())),
            );
        } else {
            eprintln!("Warning: ignoring malformed variable '{var_str}' (expected KEY=VALUE)");
        }
    }

    // 5. Create executor and scheduler.
    let executor = Arc::new(DefaultStepExecutor::new(
        Arc::new(gateway),
        Arc::new(tool_registry),
    ));
    let scheduler = Scheduler::new(executor);

    // 6. Set up the live progress display.
    let progress = Arc::new(std::sync::Mutex::new(LiveProgress::new(
        &agent.steps,
        &agent.name,
    )));

    // Channel for scheduler events.
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<SchedulerEvent>();

    // Signal to stop the render loop.
    let render_done = Arc::new(tokio::sync::Notify::new());

    // Initial render.
    {
        let mut p = progress.lock().unwrap();
        p.start();
        p.render();
    }

    // Spawn background render loop (ticks every 80ms to animate spinners).
    let render_progress = Arc::clone(&progress);
    let render_done_clone = Arc::clone(&render_done);
    let render_handle = tokio::spawn(async move {
        let tick_interval = {
            let p = render_progress.lock().unwrap();
            p.tick_interval()
        };

        loop {
            tokio::select! {
                _ = tokio::time::sleep(tick_interval) => {
                    // Drain any pending events.
                    while let Ok(event) = event_rx.try_recv() {
                        let mut p = render_progress.lock().unwrap();
                        p.on_event(&event);
                    }

                    // Redraw with updated spinner and elapsed times.
                    let mut p = render_progress.lock().unwrap();
                    if p.has_running() {
                        p.render();
                    }
                }
                Some(event) = event_rx.recv() => {
                    let mut p = render_progress.lock().unwrap();
                    p.on_event(&event);
                    p.render();
                }
                _ = render_done_clone.notified() => {
                    // Drain remaining events.
                    while let Ok(event) = event_rx.try_recv() {
                        let mut p = render_progress.lock().unwrap();
                        p.on_event(&event);
                    }
                    break;
                }
            }
        }
    });

    // 7. Run the scheduler with event callback.
    let event_tx_clone = event_tx.clone();
    let result = scheduler
        .run_with_events(&agent.steps, ctx, move |event| {
            event_tx_clone.send(event).ok();
        })
        .await;

    // Drop the sender so the render loop can drain remaining events.
    drop(event_tx);

    // Signal render loop to stop and wait for it.
    render_done.notify_one();
    render_handle.await.ok();

    // 8. Render final state.
    {
        let p = progress.lock().unwrap();
        p.render_final();
    }

    match result {
        Ok(result_ctx) => {
            println!("{}  Workflow completed successfully!\n", "✓".green().bold());

            // Print step outputs.
            println!("{}", "── Step Outputs ──".bold());
            for (step_id, value) in &result_ctx.step_outputs {
                let json = serde_json::to_string_pretty(value.inner())?;
                println!("\n  [{}]:", step_id.clone().cyan());
                for line in json.lines() {
                    println!("    {line}");
                }
            }
        }
        Err(_) => {
            // Details already printed by render_final's "Failure Details" section.
            return Err(anyhow::anyhow!(""));
        }
    }

    Ok(())
}
