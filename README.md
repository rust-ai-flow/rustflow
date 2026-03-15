<p align="center">
  <img src="./logo/rustflow-logo-full.svg" alt="RustFlow Logo" width="300">
</p>

<h1 align="center">RustFlow</h1>

<p align="center">
  High-performance AI Agent orchestration runtime — built in Rust.
</p>

<p align="center">
  <a href="./README.zh-CN.md">中文</a> ·
  <a href="#quick-start">Quick Start</a> ·
  <a href="#web-playground">Playground</a> ·
  <a href="#http-api">API</a> ·
  <a href="#typescript-sdk">TypeScript SDK</a> ·
  <a href="#architecture">Architecture</a>
</p>

---

## Why RustFlow

Python-based agent frameworks (LangChain, AutoGen) consume **100–300 MB per agent** and hit concurrency walls fast. RustFlow rewrites the orchestration layer in Rust:

| | Python (LangChain) | RustFlow |
|---|---|---|
| Memory / agent | ~200 MB | **~3 MB** |
| Concurrent agents | ~100 | **10,000+** |
| Cold start | 2–5 s | **< 50 ms** |
| Deployment | 500 MB Docker image | **5 MB single binary** |
| Infrastructure cost (1k agents) | ~$2,000 / mo | **~$500 / mo** |

## Benchmarks

Results measured on release builds (`cargo bench` / `cargo test --release`), Apple M-series, tokio multi-thread runtime.

### Concurrency — scheduler throughput

| Scenario | Agents | Steps / agent | Wall time |
|---|---|---|---|
| Single-step agents | 10,000 | 1 | **48 ms** |
| Parallel workflow | 1,000 | 10 (all independent) | **36 ms** |
| Linear chain | 500 | 20 (sequential) | **25 ms** |

### DAG parse latency (µs, median)

| Topology | 100 steps | 1,000 steps | 5,000 steps |
|---|---|---|---|
| Linear chain | ~8 µs | ~90 µs | ~490 µs |
| Fully parallel | ~5 µs | ~55 µs | ~290 µs |
| Diamond (N branches) | ~9 µs | — | — |

### Core type operations (ns, median)

| Operation | 10 items | 100 items | 1,000 items |
|---|---|---|---|
| `Agent::new` | ~800 ns | ~7 µs | ~70 µs |
| Agent serialize | ~1 µs | ~10 µs | ~100 µs |
| Agent deserialize | ~2 µs | ~18 µs | ~180 µs |
| `Context::set_step_output` × N | ~600 ns | ~5 µs | ~55 µs |

Reproduce locally:

```bash
# Micro-benchmarks (HTML report → target/criterion/)
cargo bench -p rustflow-benches

# Concurrency stress tests
cargo test -p rustflow-benches --test concurrency --release -- --nocapture
```

---

## Quick Start

### Install

```bash
curl -fsSL https://raw.githubusercontent.com/rust-ai-flow/rustflow/master/install.sh | bash
```

Builds from source, installs to `~/.rustflow/bin`, and updates your shell `PATH`. Requires Cargo (the installer will offer to install rustup if missing).

```bash
# Verify
rustflow --version
rustflow doctor --full
```

Or build manually:

```bash
git clone https://github.com/rust-ai-flow/rustflow.git
cd rustflow
cargo build --release -p rustflow-cli
# binary at target/release/rustflow
```

### Write a Workflow

```yaml
# research.yaml
name: research-and-summarize
description: Fetch a page and summarize it with a local LLM

steps:
  - id: fetch
    name: Fetch Page
    tool:
      name: http
      input:
        url: "{{vars.url}}"
        method: GET

  - id: summarize
    name: Summarize
    depends_on: [fetch]
    llm:
      provider: ollama
      model: llama3.2
      prompt: |
        Summarize this in 3 bullet points:
        {{steps.fetch.output.body}}
    retry:
      kind: exponential
      max_retries: 3
      initial_ms: 1000
```

### Run

```bash
# Execute a workflow
rustflow run research.yaml --vars '{"url":"https://example.com"}'

# Watch for file changes and re-run automatically
rustflow dev research.yaml --vars '{"url":"https://example.com"}'

# Start the API server
rustflow serve

# Open the Web Playground
rustflow playground
```

---

## CLI

```
rustflow <command> [options]
```

| Command | Description |
|---|---|
| `run <file>` | Execute a YAML workflow, stream live progress |
| `run <file> --watch` | Re-run on file change (alias for `dev`) |
| `dev <file>` | Watch mode — re-runs on every save |
| `serve` | Start the HTTP + WebSocket API server (port 18790) |
| `playground` | Start the server and open the Web Playground |
| `doctor` | Check environment (Rust, Ollama, API keys, server) |
| `doctor --full` | Include provider and server reachability checks |
| `init [name]` | Scaffold a new project with sample workflow |

### `rustflow run` — Live Progress Display

```
  ╔═══ Workflow: research-and-summarize (2 steps) ═══╗

  ┌─ Layer 1 ─────────────────────────────────────────
  │  ✓ Fetch Page [http] (342ms)
  └────────────────────────────────────────────────────
                   │
                   ▼
  ┌─ Layer 2 ─────────────────────────────────────────
  │  ⠹ Summarize [ollama/llama3.2] 4.1s
  └────────────────────────────────────────────────────
```

All steps update in-place — no scrolling noise.

### `rustflow doctor`

```
  ⚕  RustFlow Doctor

  [✓] Rust toolchain        rustc 1.82.0
  [✓] rustflow binary       v0.1.0
  [✓] .env file             found in current directory
  [✓] Ollama                v0.5.1 at localhost:11434
  [!] OPENAI_API_KEY        not set → platform.openai.com
  [✓] RustFlow server       running v0.1.0 at localhost:18790
```

---

## Web Playground

```bash
rustflow playground
# Opens http://localhost:5173/playground/
```

A three-panel web UI for writing, running, and debugging workflows interactively.

**Features:**

- **YAML editor** with syntax highlighting — edit and run workflows directly in the browser
- **Live execution panel** — per-step status cards with animated spinners and elapsed time
- **Concurrent runs** — run multiple workflows simultaneously; switch between them without interrupting any
- **Persistent history** — execution data survives page refresh via localStorage; the browser auto-reconnects to any workflow still running on the server
- **Workflow sidebar** — lists all registered agents; a pulsing dot marks actively running ones
- **Input variables** — pass `vars` as JSON from the header bar before hitting Run

![Playground screenshot](./docs/playground.png)

---

## HTTP API

Default port: **18790**

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check + version |
| `/agents` | POST | Create an agent from a step list |
| `/agents` | GET | List all agents |
| `/agents/{id}` | GET | Get agent details and steps |
| `/agents/{id}` | DELETE | Delete an agent |
| `/agents/{id}/run` | POST | Execute and wait for the final result |
| `/agents/{id}/stream` | **WS** | Execute and stream real-time events |
| `/agents/{id}/observe` | **WS** | Attach to an existing run as a read-only observer |
| `/playground/agents` | POST | Create an agent from YAML (used by the Playground) |

### WebSocket Streaming

#### `/agents/{id}/stream` — Start or join a run

Connect, send the start message, receive events:

```json
// Client → Server (once, on connect)
{ "vars": { "url": "https://example.com" } }

// Server → Client (streamed)
{"type":"step_started",   "step_id":"fetch","step_name":"Fetch Page"}
{"type":"step_succeeded", "step_id":"fetch","step_name":"Fetch Page","elapsed_ms":342,"output":{…}}
{"type":"step_started",   "step_id":"summarize","step_name":"Summarize"}
{"type":"step_retrying",  "step_id":"summarize","step_name":"Summarize","attempt":2}
{"type":"step_succeeded", "step_id":"summarize","elapsed_ms":4100,"output":"…"}
{"type":"workflow_completed","outputs":{"fetch":{…},"summarize":"…"}}
```

If a run is already active for this agent, the new connection attaches as an observer — **no duplicate execution is started**.

#### `/agents/{id}/observe` — Reconnect to an ongoing run

Same event protocol as `/stream`. Replays all events emitted so far, then streams live until the workflow finishes. If no active run exists, returns `workflow_failed` immediately.

```
Use this endpoint to reconnect after a page refresh or network drop.
```

**Event types:**

| Type | Fields |
|---|---|
| `step_started` | `step_id`, `step_name` |
| `step_succeeded` | `step_id`, `step_name`, `elapsed_ms`, `output` |
| `step_failed` | `step_id`, `step_name`, `error`, `will_retry`, `attempt`, `elapsed_ms` |
| `step_retrying` | `step_id`, `step_name`, `attempt` |
| `circuit_breaker_opened` | `resource` |
| `circuit_breaker_closed` | `resource` |
| `workflow_completed` | `outputs` |
| `workflow_failed` | `error` |

---

## TypeScript SDK

```bash
npm install rustflow
```

```ts
import { RustFlowClient, llmStep, toolStep } from 'rustflow';

const client = new RustFlowClient({ baseUrl: 'http://localhost:18790' });

// Create an agent from steps
const { id } = await client.createAgent({
  name: 'my-workflow',
  steps: [
    toolStep('fetch', 'Fetch Page', 'http', { url: 'https://example.com', method: 'GET' }),
    llmStep('summarize', 'Summarize', {
      provider: 'ollama',
      model: 'llama3.2',
      prompt: 'Summarize: {{steps.fetch.output.body}}',
    }, { depends_on: ['fetch'] }),
  ],
});

// Stream execution events
for await (const event of client.stream(id, { vars: {} })) {
  if (event.type === 'step_succeeded') {
    console.log(event.step_name, event.output);
  }
}

// Reconnect to an ongoing run (e.g. after page refresh)
for await (const event of client.observe(id)) {
  console.log(event.type);
}
```

### API Reference

| Method | Description |
|---|---|
| `health()` | `GET /health` |
| `createAgent(req)` | `POST /agents` |
| `listAgents()` | `GET /agents` |
| `getAgent(id)` | `GET /agents/:id` |
| `deleteAgent(id)` | `DELETE /agents/:id` |
| `runAgent(id, vars?)` | `POST /agents/:id/run` — blocking |
| `stream(id, options?)` | `WS /agents/:id/stream` — async generator |
| `observe(id, options?)` | `WS /agents/:id/observe` — async generator, reconnect |
| `createFromYaml(yaml)` | `POST /playground/agents` |

`stream()` and `observe()` both return `AsyncGenerator<WsEvent, WorkflowCompletedEvent | WorkflowFailedEvent>`.

---

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                        API Layer                          │
│   HTTP/REST (axum)  ·  WebSocket streaming  ·  SDKs      │
├──────────────────────────────────────────────────────────┤
│                   Orchestration Layer                     │
│   DAG parser  ·  Async scheduler  ·  Event broadcast     │
├──────────────────────────────────────────────────────────┤
│                    Execution Layer                        │
│   LLM gateway  ·  Tool engine  ·  Retry / Circuit breaker│
├──────────────────────────────────────────────────────────┤
│                   Foundation Layer                        │
│   tokio  ·  tracing  ·  serde  ·  wasmtime (WASM)        │
└──────────────────────────────────────────────────────────┘
```

### Workspace

```
crates/
  rustflow-core/          Agent, Step, Context, RetryPolicy, CircuitBreaker
  rustflow-orchestrator/  DAG parser, async scheduler, event callbacks
  rustflow-llm/           LLM gateway: OpenAI, Anthropic, Ollama
  rustflow-tools/         Tool trait + 7 built-in tools
  rustflow-plugins/       WASM plugin loader (wasmtime sandbox)
  rustflow-server/        axum HTTP + WebSocket server, run store
  rustflow-cli/           CLI binary
apps/
  playground/             React + TypeScript + Tailwind web UI
sdks/
  typescript/             TypeScript SDK (npm: rustflow)
  python/                 Python bindings (PyO3, planned)
```

---

## Features

### Built-in Tools

| Tool | Name | Description |
|---|---|---|
| `HttpTool` | `http` | HTTP requests — GET/POST/PUT/DELETE/PATCH, custom headers, JSON body |
| `FileReadTool` | `file_read` | Read files as UTF-8 or base64 |
| `FileWriteTool` | `file_write` | Write files, overwrite/append, auto-create directories |
| `ShellTool` | `shell` | Run shell commands, capture stdout/stderr/exit_code |
| `JsonExtractTool` | `json_extract` | Extract values from JSON with dot-path expressions |
| `EnvTool` | `env` | Read environment variables with defaults |
| `SleepTool` | `sleep` | Pause execution (rate limiting, backoff) |

### Retry Policies

```yaml
retry:
  kind: none           # no retry (default)

retry:
  kind: fixed
  max_retries: 3
  interval_ms: 2000

retry:
  kind: exponential
  max_retries: 5
  initial_ms: 500
  multiplier: 2.0      # 500 → 1000 → 2000 → 4000 ms
  max_interval_ms: 30000
```

### Circuit Breaker

Protects LLM providers and tools from cascading failures. Per-resource isolation — each provider/tool gets its own breaker.

```
Closed ──(N consecutive failures)──► Open
Open   ──(timeout elapsed)──────────► HalfOpen
HalfOpen──(M consecutive successes)──► Closed
HalfOpen──(any failure)─────────────► Open
```

Events emitted: `circuit_breaker_opened`, `circuit_breaker_closed`.

### Security Sandbox

- **Filesystem** — path canonicalization, directory whitelist, symlink rejection, sensitive path blocklist (`.ssh`, `.env`, `/etc/shadow`)
- **Shell** — disabled by default; command whitelist; strips dangerous env vars (`LD_PRELOAD`, etc.)
- **Environment** — dump-all blocked; `*KEY*`, `*SECRET*`, `*TOKEN*`, `*PASSWORD*` variables return `[REDACTED]`

### WASM Plugin System

Extend RustFlow with custom tools in any language that compiles to WebAssembly:

```rust
let mut loader = PluginLoader::new();
let tools = loader.load_file("plugins/my-plugin.wasm")?;
for tool in tools {
    tool_registry.register(tool).ok();
}
```

---

## LLM Provider Setup

```bash
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
# Ollama: no config needed, connects to localhost:11434
```

---

## Development

```bash
cargo build                     # Build all crates
cargo test                      # Run all tests
cargo clippy --all-targets      # Lint
cargo fmt --all -- --check      # Format check
cargo run -- doctor --full      # Check dev environment
cargo run -- serve              # Start server on :18790
cargo run -- playground         # Start server + open Playground
```

---

## Roadmap

- [x] Core type system (Agent, Step, Context, RetryPolicy)
- [x] DAG parser with topological sort and cycle detection
- [x] Async concurrent scheduler with event callbacks
- [x] Multi-provider LLM gateway (OpenAI, Anthropic, Ollama)
- [x] 7 built-in tools (http, file_read, file_write, shell, json_extract, env, sleep)
- [x] YAML workflow definition and loading
- [x] HTTP API server (Agent CRUD + execution)
- [x] WebSocket real-time streaming (`/agents/{id}/stream`)
- [x] WebSocket observe/reconnect (`/agents/{id}/observe`)
- [x] CLI — run, serve, doctor, dev, playground, init
- [x] Live flowchart progress (DAG visualization, spinner, color-coded status)
- [x] Security sandbox (filesystem jail, shell whitelist, env redaction)
- [x] Circuit breaker (per-resource, scheduler-integrated)
- [x] WASM plugin system (wasmtime)
- [x] TypeScript SDK (`npm install rustflow`) with `stream()` and `observe()`
- [x] Web Playground (concurrent runs, localStorage persistence, auto-reconnect)
- [x] One-line install script (`curl … | bash`)
- [ ] Prometheus metrics + OpenTelemetry tracing
- [ ] Python SDK (PyO3)
- [ ] Tauri desktop app

---

## License

Apache-2.0 (core) / BSL 1.1 (enterprise features)
