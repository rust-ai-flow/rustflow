use clap::Args;
use std::fs;
use std::path::PathBuf;

/// Initialise a new RustFlow project.
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Project name (defaults to the current directory name).
    pub name: Option<String>,

    /// Directory to initialise the project in (default: current directory).
    #[arg(short, long, default_value = ".")]
    pub dir: PathBuf,

    /// Project template to use.
    #[arg(short, long, default_value = "basic")]
    pub template: String,
}

pub async fn execute(args: InitArgs) -> anyhow::Result<()> {
    let dir = args.dir.canonicalize().unwrap_or(args.dir.clone());
    let name = args.name.unwrap_or_else(|| {
        dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-rustflow-project")
            .to_string()
    });

    println!("✦  Initialising project '{name}' in {}", dir.display());

    // Create project structure.
    let workflows_dir = dir.join("workflows");
    fs::create_dir_all(&workflows_dir)?;

    // Create rustflow.toml config.
    let config_path = dir.join("rustflow.toml");
    if !config_path.exists() {
        fs::write(
            &config_path,
            format!(
                r#"# RustFlow project configuration
[project]
name = "{name}"
version = "0.1.0"

[server]
host = "127.0.0.1"
port = 18790

[llm]
# default_provider = "openai"

# [llm.openai]
# api_key = "${{OPENAI_API_KEY}}"

# [llm.anthropic]
# api_key = "${{ANTHROPIC_API_KEY}}"

[llm.ollama]
base_url = "http://localhost:11434"
"#
            ),
        )?;
        println!("   Created rustflow.toml");
    }

    // Create sample workflow based on template.
    let workflow_path = workflows_dir.join("hello.yaml");
    if !workflow_path.exists() {
        let workflow_content = match args.template.as_str() {
            "llm" => TEMPLATE_LLM,
            "pipeline" => TEMPLATE_PIPELINE,
            _ => TEMPLATE_BASIC,
        };
        fs::write(&workflow_path, workflow_content)?;
        println!("   Created workflows/hello.yaml");
    }

    // Create .env.example.
    let env_path = dir.join(".env.example");
    if !env_path.exists() {
        fs::write(
            &env_path,
            "# Copy to .env and fill in your keys\n\
             OPENAI_API_KEY=sk-...\n\
             ANTHROPIC_API_KEY=sk-ant-...\n",
        )?;
        println!("   Created .env.example");
    }

    println!("\n✓  Project '{name}' initialised!");
    println!("\n   Next steps:");
    println!("     1. Edit workflows/hello.yaml");
    println!("     2. Run: rustflow run workflows/hello.yaml");

    Ok(())
}

const TEMPLATE_BASIC: &str = r#"name: hello-world
description: A basic RustFlow workflow example

steps:
  - id: greet
    name: Fetch greeting
    tool:
      name: http
      input:
        url: "https://httpbin.org/get?greeting=hello"
        method: GET
"#;

const TEMPLATE_LLM: &str = r#"name: llm-example
description: A simple LLM workflow

steps:
  - id: generate
    name: Generate text
    llm:
      provider: ollama
      model: llama3
      prompt: "Write a haiku about programming in Rust."
      max_tokens: 200
"#;

const TEMPLATE_PIPELINE: &str = r#"name: pipeline-example
description: A multi-step pipeline with dependencies

steps:
  - id: fetch-data
    name: Fetch Data
    tool:
      name: http
      input:
        url: "https://httpbin.org/json"
        method: GET

  - id: summarise
    name: Summarise Data
    llm:
      provider: ollama
      model: llama3
      prompt: "Summarise the following JSON data:\n\n{{steps.fetch-data.output}}"
      max_tokens: 500
    depends_on:
      - fetch-data
    retry:
      kind: fixed
      max_retries: 2
      interval_ms: 2000
    timeout_ms: 60000
"#;
