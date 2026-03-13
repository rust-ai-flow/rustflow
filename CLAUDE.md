# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RustFlow is a high-performance AI Agent orchestration runtime written in Rust (Edition 2024). It aims to replace Python-based agent frameworks (LangChain, AutoGen) with a solution that uses <5MB per agent (vs 100-300MB in Python), supports 10,000+ concurrent agents on a single machine, and deploys as a single binary.

The project is in early development. The technical design document is at `rustflow.md`.

## Architecture (4 Layers)

1. **API Layer** — HTTP/gRPC (axum), WebSocket streaming, multi-language SDKs
2. **Orchestration Layer** — DAG workflow parsing, async scheduler, state machine, context management
3. **Execution Layer** — Tool engine, LLM gateway (multi-model/load balancing), retry/circuit-breaker/timeout
4. **Foundation Layer** — tokio async runtime, memory pool, observability (tracing)

## Build & Development Commands

```bash
cargo build                          # Build all crates
cargo build -p rustflow-core         # Build a single crate
cargo test                           # Run all tests
cargo test -p rustflow-core          # Run tests for a single crate
cargo test -- test_name              # Run a specific test
cargo clippy --all-targets           # Lint
cargo fmt --all -- --check           # Check formatting
cargo bench                          # Run benchmarks (in benches/)
```

## Workspace Structure (Cargo Workspace)

```
crates/
  rustflow-core/          — Core types: Agent, Step, Context, Error, Value
  rustflow-orchestrator/  — DAG parser, task scheduler, state machine
  rustflow-llm/           — LLM gateway: multi-model routing, streaming, caching
  rustflow-tools/         — Tool trait definition + built-in tools (HTTP, file, JSON)
  rustflow-plugins/       — WASM plugin loader (wasmtime sandbox)
  rustflow-server/        — axum HTTP/WebSocket server (embeds Web UI static assets)
  rustflow-cli/           — CLI binary: run, init, doctor, dev, playground commands
apps/
  playground/             — Web console (React + TypeScript + Tailwind CSS)
  desktop/                — Tauri 2.0 desktop app
sdks/
  python/                 — Python bindings (PyO3)
  typescript/             — TypeScript SDK
```

## Key Technical Decisions

- **Async runtime**: tokio (all I/O and scheduling)
- **HTTP framework**: axum (server) + reqwest (client)
- **Serialization**: serde + serde_json throughout
- **Observability**: tracing + tracing-subscriber; Prometheus metrics; OpenTelemetry
- **Plugin sandbox**: wasmtime for WASM tool plugins
- **Testing**: tokio::test for async tests, mockall for mocking
- **Config**: TOML files with env var interpolation, priority: defaults < global config < project config < env vars < CLI args
- **Web UI**: React/TS/Tailwind compiled into the Rust binary via `include_dir!`

## Core Abstractions

- **Tool trait** (`#[async_trait] pub trait Tool: Send + Sync`) — unified interface for all tools with `name()`, `description()`, `parameters()` (JSON Schema), and `execute(input, ctx)`
- **Workflow** — defined in YAML or Rust DSL (builder pattern: `AgentBuilder` / `StepBuilder`)
- **Step states**: Pending -> Running -> Success / Failed / Retrying
- **Resilience**: retry (fixed/exponential/custom), circuit breaker, timeout per step/agent/global, model fallback

## Conventions

- License: Apache-2.0 (core) / BSL 1.1 (enterprise features)
- Default server port: 18790
- Config path: `~/.rustflow/config.toml` (global), `./rustflow.toml` (project)
- All LLM API keys accessed via environment variables, never hardcoded
