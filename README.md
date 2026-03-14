<p align="center">
  <img src="./logo/rustflow-logo-full.svg" alt="RustFlow Logo" width="300">
</p>

<h1 align="center">RustFlow</h1>

<p align="center">
  High-performance AI Agent orchestration runtime — redefining agent infrastructure with Rust.
</p>

<p align="center">
  <a href="./README.zh-CN.md">中文</a> ·
  <a href="#quick-start">Quick Start</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#cli">CLI</a> ·
  <a href="#http-api">API</a> ·
  <a href="#development">Development</a>
</p>

---

## Why RustFlow

Python-based agent frameworks (LangChain, AutoGen, etc.) consume 100–300 MB per agent, limiting single-machine concurrency. RustFlow rewrites the stack from the ground up in Rust:

- **<5 MB per agent** memory footprint
- **10,000+ concurrent agents** on a single machine
- **Single binary deployment** — no Python runtime needed
- **Millisecond-level scheduling** on the tokio async runtime

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                       API Layer                          │
│  HTTP/REST (axum)  ·  WebSocket  ·  Multi-language SDKs  │
├─────────────────────────────────────────────────────────┤
│                  Orchestration Layer                      │
│  DAG workflow parser  ·  Async scheduler  ·  State machine│
├─────────────────────────────────────────────────────────┤
│                   Execution Layer                         │
│  Tool engine  ·  LLM gateway (multi-model)  ·  Retry/CB  │
├─────────────────────────────────────────────────────────┤
│                  Foundation Layer                         │
│  tokio async runtime  ·  Memory pool  ·  Observability    │
└─────────────────────────────────────────────────────────┘
```

### Workspace Structure

```
crates/
  rustflow-core/          Core types: Agent, Step, Context, Value, RetryPolicy
  rustflow-orchestrator/  DAG parser, task scheduler, step executor, flow renderer
  rustflow-llm/           LLM gateway: multi-model routing, streaming, provider abstraction
  rustflow-tools/         Tool trait + 7 built-in tools
  rustflow-plugins/       WASM plugin loader (wasmtime sandbox)
  rustflow-server/        axum HTTP server (Agent CRUD + execution API)
  rustflow-cli/           CLI: run, init, serve, doctor commands
```

## Features

### Core (`rustflow-core`)

- **Agent / Step model** — full domain model with serde serialization
- **StepKind** — LLM steps and Tool steps
- **Context** — execution context for inter-step data passing and shared variables
- **RetryPolicy** — None / Fixed / Exponential strategies with automatic backoff
- **WorkflowDef** — YAML workflow file parser, converts to Agent

### Orchestration (`rustflow-orchestrator`)

- **DAG parser** — Kahn's algorithm topological sort; detects cycles, duplicate IDs, invalid deps
- **Async scheduler** — concurrent step execution via tokio JoinSet, dependency-driven scheduling
- **Event system** — `SchedulerEvent` callbacks for real-time progress (step started/succeeded/failed/retrying)
- **Retry mechanism** — automatic retry with configurable backoff on step failure
- **Template interpolation** — `{{steps.<id>.output}}` and `{{vars.<key>}}` variable substitution
- **Flow renderer** — computes execution layers from DAG, renders parallel/serial flowchart

### LLM Gateway (`rustflow-llm`)

- **Multi-provider routing** — register multiple providers, route by name
- **OpenAI provider** — complete + stream, OpenAI API compatible
- **Anthropic provider** — complete + stream, system prompt extraction
- **Ollama provider** — local models, no API key required
- **Streaming** — all providers support SSE streaming responses

### Tool System (`rustflow-tools`)

- **Tool trait** — unified async interface (`name`, `description`, `parameters`, `execute`)
- **ToolRegistry** — thread-safe registry with name-based lookup

7 built-in tools:

| Tool | Name | Description |
|------|------|-------------|
| **HttpTool** | `http` | HTTP requests (GET/POST/PUT/DELETE/PATCH) with custom headers, JSON body, timeout |
| **FileReadTool** | `file_read` | Read file contents as UTF-8 or base64 |
| **FileWriteTool** | `file_write` | Write to files with overwrite/append modes, auto-create parent dirs |
| **ShellTool** | `shell` | Execute shell commands, returns stdout/stderr/exit_code, supports timeout, env vars, cwd |
| **JsonExtractTool** | `json_extract` | Extract values from JSON using dot-path expressions (e.g. `users.0.name`) |
| **EnvTool** | `env` | Read environment variables, single or all, with defaults |
| **SleepTool** | `sleep` | Pause execution for specified milliseconds (rate limiting) |

### CLI (`rustflow-cli`)

| Command | Status | Description |
|---------|--------|-------------|
| `rustflow run <file>` | ✅ | Load and execute a workflow from YAML |
| `rustflow init [name]` | ✅ | Initialize project with config and sample workflow |
| `rustflow serve` | ✅ | Start the HTTP API server |
| `rustflow doctor` | ✅ | Check system dependencies (Rust, Cargo, etc.) |
| `rustflow dev` | 🚧 | Dev server with file watching + hot reload |
| `rustflow playground` | 🚧 | Web-based interactive debugging UI |

#### Live Flowchart Progress

`rustflow run` provides a Claude Code-style live terminal progress display:

- Automatically analyzes the DAG and renders a flowchart showing parallel/serial execution layers
- Running steps display animated spinners with real-time elapsed time
- Color-coded status: cyan (running) / green (success) / red (failed) / yellow (retrying)
- All steps update in-place within the same flowchart — no scrolling

```
  ╔═══ Workflow: my-workflow (8 steps) ═══╗

  ┌─ Layer 1 ── parallel (3) ─────────────────────
  │  ✓ Fetch data A [http] (0.8s)
  │  ✓ Fetch data B [http] (1.2s)
  │  ⠹ Fetch data C [http] 2.1s
  └─────────────────────────────────────────────
                 │
                 ▼
  ┌─ Layer 2 ── parallel (3) ─────────────────────
  │  ⠼ Analyze A [ollama/qwen3:8b] 12.4s
  │  ○ Analyze B [ollama/qwen3:8b]
  │  ○ Analyze C [ollama/qwen3:8b]
  └─────────────────────────────────────────────
```

### WASM Plugin System (`rustflow-plugins`)

Extend RustFlow with custom tools written in any language that compiles to WebAssembly.

**Architecture:**
- Plugins compile to `.wasm` and are loaded at runtime via `wasmtime` (JIT, sandboxed)
- Each exported tool automatically becomes a `PluginTool` implementing the `Tool` trait
- Plugin tools register into the same `ToolRegistry` as built-in tools — transparent to workflows

**Plugin ABI** (what a plugin must export):

| Export | Signature | Description |
|--------|-----------|-------------|
| `memory` | linear memory | Shared linear memory |
| `rustflow_alloc` | `(size: i32) -> i32` | Allocate bytes, return pointer |
| `rustflow_dealloc` | `(ptr: i32, size: i32)` | Free allocation |
| `rustflow_plugin_manifest` | `() -> i64` | Packed `(ptr << 32 \| len)` of manifest JSON |
| `rustflow_tool_execute` | `(name_ptr, name_len, input_ptr, input_len: i32) -> i64` | Run a tool, return packed output JSON pointer |

The plugin also imports `rustflow::log(level: i32, ptr: i32, len: i32)` for host-side logging.

**Manifest JSON** embedded in the plugin:
```json
{
  "name": "my-plugin",
  "version": "1.0.0",
  "description": "Optional description",
  "tools": [
    {
      "name": "my-tool",
      "description": "Does something useful",
      "parameters": { "type": "object", "properties": { "input": { "type": "string" } } }
    }
  ]
}
```

**Usage:**
```rust
let mut loader = PluginLoader::new();
let tools = loader.load_file("plugins/my-plugin.wasm")?;
for tool in tools {
    tool_registry.register(tool).ok();
}
```

### Security Sandbox (`SecurityPolicy`)

RustFlow enforces a configurable security policy across all tool execution to prevent path traversal, command injection, and credential leakage.

**Filesystem Sandbox** (`file_read` / `file_write`):
- Path canonicalization — resolves `..` and relative paths before access
- Allowed directory whitelist — restricts file access to specified directories
- Symlink rejection — symlinks are blocked by default to prevent escape
- Sensitive path blocklist — `.ssh`, `.env`, `/etc/shadow`, etc. are always denied
- Write size limit — prevents memory/disk exhaustion (default 50 MB)

**Shell Sandbox** (`shell`):
- Disabled by default — must be explicitly enabled in policy
- Command whitelist — restrict execution to approved commands only
- Dangerous env key filtering — `LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`, etc. are stripped
- Output truncation — prevents memory exhaustion from unbounded output (default 1 MB)
- Timeout clamping — enforces a maximum timeout regardless of step config (default 300s)

**Environment Variable Protection** (`env`):
- Dump-all disabled — reading all env vars at once is blocked by default
- Sensitive value redaction — variables matching `*KEY*`, `*SECRET*`, `*PASSWORD*`, `*TOKEN*` patterns return `[REDACTED]`

Policy is configured per-execution and passed to tools at construction time. See `SecurityPolicy` in `rustflow-tools` for full configuration options.

### Circuit Breaker (`rustflow-core`)

RustFlow's circuit breaker protects LLM providers and tools from cascading failures. It is integrated directly into the scheduler — no changes to your workflow YAML needed.

**State machine:**

```
Closed ──(failure_threshold consecutive failures)──► Open
Open   ──(timeout_ms elapsed)──────────────────────► HalfOpen
HalfOpen──(success_threshold consecutive successes)─► Closed
HalfOpen──(any failure)────────────────────────────► Open
```

**Per-resource isolation:** each LLM provider name (e.g. `openai`, `ollama`) and tool name (e.g. `http`, `shell`) gets its own independent breaker.

**Configuration:**

```rust
CircuitBreakerConfig {
    failure_threshold: 5,   // consecutive failures before opening
    success_threshold: 2,   // consecutive successes in HalfOpen to close
    timeout_ms: 30_000,     // time (ms) to stay Open before probing
}
```

**Usage:**

```rust
let registry = Arc::new(CircuitBreakerRegistry::with_default_config(
    CircuitBreakerConfig {
        failure_threshold: 3,
        success_threshold: 1,
        timeout_ms: 10_000,
    },
));

let scheduler = Scheduler::new(executor)
    .with_circuit_breaker(registry);
```

**Events emitted:**

| Event | Trigger |
|-------|---------|
| `CircuitBreakerOpened { resource }` | Circuit transitions to Open (or pre-open check blocks a step) |
| `CircuitBreakerClosed { resource }` | Circuit transitions from HalfOpen back to Closed |

### HTTP Server (`rustflow-server`)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/agents` | POST | Create an agent |
| `/agents` | GET | List all agents |
| `/agents/{id}` | GET | Get agent details |
| `/agents/{id}` | DELETE | Delete an agent |
| `/agents/{id}/run` | POST | Execute an agent workflow (blocking, returns outputs) |
| `/agents/{id}/stream` | **WS** | Execute an agent with real-time event streaming |
| `/playground` | GET | Web Playground UI |
| `/playground/agents` | POST | Create an agent from YAML (used by the Playground) |

### WebSocket Streaming (`/agents/{id}/stream`)

Connect via WebSocket to stream live execution events as JSON frames.

**Client → Server (one message to start):**
```json
{ "vars": { "topic": "Rust", "lang": "English" } }
```

**Server → Client (streamed frames):**
```json
{"type":"step_started",   "step_id":"fetch","step_name":"Fetch Data"}
{"type":"step_succeeded", "step_id":"fetch","step_name":"Fetch Data","elapsed_ms":820,"output":{…}}
{"type":"step_failed",    "step_id":"s2","step_name":"Summarise","error":"…","will_retry":true,"attempt":1,"elapsed_ms":12}
{"type":"step_retrying",  "step_id":"s2","step_name":"Summarise","attempt":2}
{"type":"circuit_breaker_opened","resource":"ollama"}
{"type":"circuit_breaker_closed","resource":"ollama"}
{"type":"workflow_completed","outputs":{"fetch":{…},"summarise":"…"}}
{"type":"workflow_failed","error":"step 'fetch' failed after all retries"}
```

After `workflow_completed` or `workflow_failed` the server closes the connection.

## Quick Start

### Install

```bash
# Build from source
git clone https://github.com/rust-ai-flow/rustflow.git
cd rustflow
cargo build --release

# Binary at target/release/rustflow
```

### Initialize a Project

```bash
mkdir my-project && cd my-project
rustflow init my-project

# Creates:
#   rustflow.toml          Project config
#   workflows/hello.yaml   Sample workflow
#   .env.example           Env var template
```

### Write a Workflow

```yaml
# workflows/hello.yaml
name: hello-agent
description: Fetch data and summarize with LLM

steps:
  - id: fetch
    name: Fetch Data
    tool:
      name: http
      input:
        url: "https://httpbin.org/json"
        method: GET

  - id: save_raw
    name: Save Raw Data
    tool:
      name: file_write
      input:
        path: "./output/raw.json"
        content: "{{steps.fetch.output}}"
    depends_on:
      - fetch

  - id: summarise
    name: Summarise
    llm:
      provider: ollama
      model: llama3
      prompt: "Summarise: {{steps.fetch.output}}"
      max_tokens: 500
    depends_on:
      - fetch
    retry:
      kind: fixed
      max_retries: 2
      interval_ms: 2000
```

### Run

```bash
# Execute a workflow
rustflow run workflows/hello.yaml

# Pass variables
rustflow run workflows/hello.yaml --var topic=Rust --var lang=English

# Start the API server
rustflow serve --host 0.0.0.0 --port 18790
```

### Execute via API

```bash
# Create an agent
curl -X POST http://localhost:18790/agents \
  -H "Content-Type: application/json" \
  -d @workflows/agent.json

# Run an agent
curl -X POST http://localhost:18790/agents/{id}/run \
  -H "Content-Type: application/json" \
  -d '{"vars": {"topic": "Rust"}}'
```

## Tech Stack

| Component | Choice |
|-----------|--------|
| Async runtime | tokio |
| HTTP framework | axum (server) + reqwest (client) |
| Serialization | serde + serde_json + serde_yaml |
| Terminal UI | crossterm (colors, cursor control, spinner animation) |
| Observability | tracing + tracing-subscriber |
| CLI parsing | clap (derive) |
| Error handling | thiserror + anyhow |
| Plugin sandbox | wasmtime (planned) |
| Testing | tokio::test, 367 unit tests |

## Development

```bash
cargo build                          # Build all crates
cargo test                           # Run all tests (367)
cargo clippy --all-targets           # Lint
cargo fmt --all -- --check           # Check formatting
cargo run -- doctor                  # Check dev environment
cargo run -- serve                   # Start dev server
```

### LLM Provider Setup

Configure API keys via environment variables — available providers are auto-registered at runtime:

```bash
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
# Ollama connects to localhost:11434 by default, no config needed
```

## Roadmap

- [x] Core type system (Agent, Step, Context, Value)
- [x] DAG workflow parsing with topological sort
- [x] Async concurrent scheduler (dependency-driven + retry + event callbacks)
- [x] Multi-provider LLM gateway (OpenAI, Anthropic, Ollama)
- [x] Tool system + 7 built-in tools (http, file_read, file_write, shell, json_extract, env, sleep)
- [x] YAML workflow definition and loading
- [x] HTTP API server (Agent CRUD + execution)
- [x] CLI tools (run, init, serve, doctor)
- [x] Live flowchart progress display (DAG visualization, spinner animation, color-coded status)
- [x] Security sandbox (filesystem jail, shell whitelist, env redaction)
- [x] WASM plugin system (wasmtime)
- [x] Circuit breaker (Closed/Open/HalfOpen state machine, per-resource, scheduler-integrated)
- [x] WebSocket real-time streaming (`/agents/{id}/stream`, per-step event frames + final output)
- [x] Web Playground UI (single-file HTML + React/TS source, embedded in `/playground`)
- [ ] Python SDK (PyO3)
- [ ] TypeScript SDK
- [ ] Prometheus metrics + OpenTelemetry
- [ ] Tauri desktop app

## License

Apache-2.0 (core) / BSL 1.1 (enterprise features)
