<p align="center">
  <img src="./logo/rustflow-logo-full.svg" alt="RustFlow Logo" width="300">
</p>

<h1 align="center">RustFlow</h1>

<p align="center">
  高性能 AI Agent 编排运行时，用 Rust 重新定义 Agent 基础设施。
</p>

<p align="center">
  <a href="#快速开始">快速开始</a> ·
  <a href="#架构概览">架构</a> ·
  <a href="#cli-命令">CLI</a> ·
  <a href="#http-api">API</a> ·
  <a href="#开发">开发</a>
</p>

---

## 为什么选择 RustFlow

Python Agent 框架（LangChain、AutoGen 等）每个 Agent 占用 100–300 MB 内存，单机并发上限低。RustFlow 用 Rust 从底层重写，目标是：

- **<5 MB / Agent** 内存占用
- **10,000+ 并发 Agent** 单机运行
- **单二进制部署**，无需 Python 运行时
- **毫秒级调度**，基于 tokio 异步运行时

## 技术架构

```
┌─────────────────────────────────────────────────────────┐
│                      API Layer                          │
│  HTTP/REST (axum)  ·  WebSocket  ·  多语言 SDK          │
├─────────────────────────────────────────────────────────┤
│                  Orchestration Layer                     │
│  DAG 工作流解析  ·  异步调度器  ·  状态机  ·  上下文管理   │
├─────────────────────────────────────────────────────────┤
│                   Execution Layer                        │
│  Tool 引擎  ·  LLM 网关 (多模型/负载均衡)  ·  重试/熔断   │
├─────────────────────────────────────────────────────────┤
│                  Foundation Layer                        │
│  tokio 异步运行时  ·  内存池  ·  可观测性 (tracing)       │
└─────────────────────────────────────────────────────────┘
```

### Workspace 结构

```
crates/
  rustflow-core/          核心类型：Agent, Step, Context, Value, RetryPolicy
  rustflow-orchestrator/  DAG 解析器、任务调度器、步骤执行器
  rustflow-llm/           LLM 网关：多模型路由、流式输出、Provider 抽象
  rustflow-tools/         Tool trait 定义 + 内置工具（HTTP 等）
  rustflow-plugins/       WASM 插件加载器（wasmtime 沙箱）
  rustflow-server/        axum HTTP 服务器（Agent CRUD + 执行 API）
  rustflow-cli/           CLI：run, init, serve, doctor 等命令
```

## 已实现功能

### Core (`rustflow-core`)

- **Agent / Step 模型** — 完整的领域模型，支持 serde 序列化
- **StepKind** — LLM 步骤和 Tool 步骤两种类型
- **Context** — 执行上下文，Step 间数据传递和共享变量
- **RetryPolicy** — 支持 None / Fixed / Exponential 三种重试策略，自动计算退避时间
- **WorkflowDef** — YAML 工作流文件解析，自动转换为 Agent

### Orchestration (`rustflow-orchestrator`)

- **DAG 解析器** — Kahn 算法拓扑排序，检测环路、重复 ID、无效依赖
- **异步调度器** — 基于 tokio JoinSet 并发执行就绪步骤，依赖驱动调度
- **重试机制** — 步骤失败后自动按 RetryPolicy 重试，含退避等待
- **DefaultStepExecutor** — 将 LLM 步骤路由到 LlmGateway，Tool 步骤路由到 ToolRegistry
- **模板插值** — 支持 `{{steps.<id>.output}}` 和 `{{vars.<key>}}` 变量替换

### LLM Gateway (`rustflow-llm`)

- **多 Provider 路由** — 注册多个 Provider，按名称路由请求
- **OpenAI Provider** — 完整实现 complete + stream，兼容 OpenAI API 格式
- **Anthropic Provider** — 完整实现 complete + stream，支持 system prompt 提取
- **Ollama Provider** — 本地模型支持，无需 API Key
- **流式输出** — 所有 Provider 均支持 SSE 流式响应

### Tool System (`rustflow-tools`)

- **Tool trait** — 统一的异步工具接口 (`name`, `description`, `parameters`, `execute`)
- **ToolRegistry** — 线程安全的工具注册表，按名称查找
- **HttpTool** — 内置 HTTP 工具，支持 GET/POST/PUT/DELETE/PATCH、自定义 Header、超时控制

### HTTP Server (`rustflow-server`)

| 端点 | 方法 | 说明 |
|------|------|------|
| `/health` | GET | 健康检查 |
| `/agents` | POST | 创建 Agent |
| `/agents` | GET | 列出所有 Agent |
| `/agents/{id}` | GET | 获取 Agent 详情 |
| `/agents/{id}` | DELETE | 删除 Agent |
| `/agents/{id}/run` | POST | 执行 Agent 工作流 |

### CLI (`rustflow-cli`)

| 命令 | 状态 | 说明 |
|------|------|------|
| `rustflow run <file>` | ✅ | 从 YAML 文件加载并执行工作流 |
| `rustflow init [name]` | ✅ | 初始化项目，生成配置和示例工作流 |
| `rustflow serve` | ✅ | 启动 HTTP API 服务器 |
| `rustflow doctor` | ✅ | 检查系统依赖（Rust、Cargo 等） |
| `rustflow dev` | 🚧 | 开发服务器（文件监听 + 热重载） |
| `rustflow playground` | 🚧 | Web 交互式调试界面 |

## 快速开始

### 安装

```bash
# 从源码构建
git clone https://github.com/rustflow/rustflow.git
cd rustflow
cargo build --release

# 二进制位于 target/release/rustflow
```

### 初始化项目

```bash
mkdir my-project && cd my-project
rustflow init my-project

# 生成：
#   rustflow.toml          项目配置
#   workflows/hello.yaml   示例工作流
#   .env.example           环境变量模板
```

### 编写工作流

```yaml
# workflows/hello.yaml
name: hello-agent
description: 获取数据并用 LLM 总结

steps:
  - id: fetch
    name: Fetch Data
    tool:
      name: http
      input:
        url: "https://httpbin.org/json"
        method: GET

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

### 运行

```bash
# 执行工作流
rustflow run workflows/hello.yaml

# 传入变量
rustflow run workflows/hello.yaml --var topic=Rust --var lang=Chinese

# 启动 API 服务器
rustflow serve --host 0.0.0.0 --port 18790
```

### 通过 API 执行

```bash
# 创建 Agent
curl -X POST http://localhost:18790/agents \
  -H "Content-Type: application/json" \
  -d @workflows/agent.json

# 执行 Agent
curl -X POST http://localhost:18790/agents/{id}/run \
  -H "Content-Type: application/json" \
  -d '{"vars": {"topic": "Rust"}}'
```

## 技术栈

| 组件 | 选型 |
|------|------|
| 异步运行时 | tokio |
| HTTP 框架 | axum (服务端) + reqwest (客户端) |
| 序列化 | serde + serde_json + serde_yaml |
| 可观测性 | tracing + tracing-subscriber |
| CLI 解析 | clap (derive) |
| 错误处理 | thiserror + anyhow |
| 插件沙箱 | wasmtime (规划中) |
| 测试 | tokio::test, 内置单元测试 |

## 开发

```bash
cargo build                          # 构建所有 crate
cargo test                           # 运行所有测试
cargo clippy --all-targets           # Lint 检查
cargo fmt --all -- --check           # 格式检查
cargo run -- doctor                  # 检查开发环境
cargo run -- serve                   # 启动开发服务器
```

### LLM Provider 配置

通过环境变量配置 API Key，在执行时自动注册可用的 Provider：

```bash
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
# Ollama 默认连接 localhost:11434，无需配置
```

## 路线图

- [x] 核心类型系统 (Agent, Step, Context, Value)
- [x] DAG 工作流解析与拓扑排序
- [x] 异步并发调度器（依赖驱动 + 重试）
- [x] LLM 多 Provider 网关 (OpenAI, Anthropic, Ollama)
- [x] Tool 系统 + 内置 HTTP 工具
- [x] YAML 工作流定义与加载
- [x] HTTP API 服务器 (Agent CRUD + 执行)
- [x] CLI 工具 (run, init, serve, doctor)
- [ ] WASM 插件系统集成 (wasmtime)
- [ ] 熔断器 (Circuit Breaker)
- [ ] WebSocket 实时流式输出
- [ ] Web Playground 交互界面
- [ ] Python SDK (PyO3)
- [ ] TypeScript SDK
- [ ] Prometheus 指标 + OpenTelemetry
- [ ] Tauri 桌面应用

## 许可

Apache-2.0 (核心) / BSL 1.1 (企业功能)
