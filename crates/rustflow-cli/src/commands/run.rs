use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use tracing::info;

use rustflow_core::context::Context;
use rustflow_core::types::Value;
use rustflow_core::workflow::WorkflowDef;
use rustflow_llm::LlmGateway;
use rustflow_llm::providers::anthropic::AnthropicProvider;
use rustflow_llm::providers::ollama::OllamaProvider;
use rustflow_llm::providers::openai::OpenAiProvider;
use rustflow_orchestrator::{DefaultStepExecutor, Scheduler};
use rustflow_tools::HttpTool;
use rustflow_tools::registry::ToolRegistry;

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
    println!("▶  Loading workflow: {}", args.file.display());

    // 1. Parse the workflow YAML.
    let workflow = WorkflowDef::from_file(&args.file).map_err(|e| anyhow::anyhow!("{e}"))?;
    let agent = workflow.into_agent().map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("   Agent: {} ({} steps)", agent.name, agent.steps.len());

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
    // Ollama is always available (local, no API key needed).
    gateway.register(OllamaProvider::default());
    info!("registered Ollama provider");

    // 3. Set up the tool registry with built-in tools.
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(HttpTool::new()).ok();

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

    // 5. Create executor and scheduler, then run.
    let executor = Arc::new(DefaultStepExecutor::new(
        Arc::new(gateway),
        Arc::new(tool_registry),
    ));
    let scheduler = Scheduler::new(executor);

    println!("▶  Executing...\n");
    let result_ctx = scheduler
        .run(&agent.steps, ctx)
        .await
        .map_err(|e| anyhow::anyhow!("execution failed: {e}"))?;

    // 6. Print results.
    println!("\n✓  Workflow completed successfully!\n");
    println!("── Step Outputs ──");
    for (step_id, value) in &result_ctx.step_outputs {
        let json = serde_json::to_string_pretty(value.inner())?;
        println!("\n  [{step_id}]:");
        for line in json.lines() {
            println!("    {line}");
        }
    }

    Ok(())
}
