<p align="center">
  <img src="./logo/rustflow-logo-full.svg" alt="RustFlow Logo" width="300">
</p>

<h1 align="center">RustFlow</h1>

<p align="center">
  高性能 AI Agent 编排运行时，用 Rust 重新定义 Agent 基础设施。
</p>

<p align="center">
  <a href="./README.md">English</a> ·
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
  rustflow-orchestrator/  DAG 解析器、任务调度器、步骤执行器、流程图渲染
  rustflow-llm/           LLM 网关：多模型路由、流式输出、Provider 抽象
  rustflow-tools/         Tool trait 定义 + 7 个内置工具
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
- **事件回调** — `SchedulerEvent` 系统，支持实时进度汇报（步骤启动/成功/失败/重试）
- **重试机制** — 步骤失败后自动按 RetryPolicy 重试，含退避等待
- **DefaultStepExecutor** — 将 LLM 步骤路由到 LlmGateway，Tool 步骤路由到 ToolRegistry
- **模板插值** — 支持 `{{steps.<id>.output}}` 和 `{{vars.<key>}}` 变量替换
- **流程图渲染** — 自动计算执行层级，渲染 DAG 流程图，展示并行/串行关系

### LLM Gateway (`rustflow-llm`)

- **多 Provider 路由** — 注册多个 Provider，按名称路由请求
- **OpenAI Provider** — 完整实现 complete + stream，兼容 OpenAI API 格式
- **Anthropic Provider** — 完整实现 complete + stream，支持 system prompt 提取
- **Ollama Provider** — 本地模型支持，无需 API Key
- **流式输出** — 所有 Provider 均支持 SSE 流式响应

### Tool System (`rustflow-tools`)

- **Tool trait** — 统一的异步工具接口 (`name`, `description`, `parameters`, `execute`)
- **ToolRegistry** — 线程安全的工具注册表，按名称查找

内置 7 个工具：

| 工具 | 注册名 | 说明 |
|------|--------|------|
| **HttpTool** | `http` | HTTP 请求 (GET/POST/PUT/DELETE/PATCH)，自定义 Header、JSON Body、超时 |
| **FileReadTool** | `file_read` | 读取文件内容，支持 UTF-8 和 base64 编码 |
| **FileWriteTool** | `file_write` | 写入文件，支持覆盖/追加模式，自动创建父目录 |
| **ShellTool** | `shell` | 执行 Shell 命令，返回 stdout/stderr/exit_code，支持超时、环境变量、工作目录 |
| **JsonExtractTool** | `json_extract` | 用点路径从 JSON 提取值 (如 `users.0.name`)，支持默认值 |
| **EnvTool** | `env` | 读取环境变量，支持单个或全部，带默认值 |
| **SleepTool** | `sleep` | 暂停执行指定毫秒数，用于速率限制 |

### CLI (`rustflow-cli`)

| 命令 | 状态 | 说明 |
|------|------|------|
| `rustflow run <file>` | ✅ | 从 YAML 文件加载并执行工作流 |
| `rustflow init [name]` | ✅ | 初始化项目，生成配置和示例工作流 |
| `rustflow serve` | ✅ | 启动 HTTP API 服务器 |
| `rustflow doctor` | ✅ | 检查系统依赖（Rust、Cargo 等） |
| `rustflow dev` | 🚧 | 开发服务器（文件监听 + 热重载） |
| `rustflow playground` | 🚧 | Web 交互式调试界面 |

#### 实时流程图进度

`rustflow run` 命令提供 Claude Code 风格的终端实时进度显示：

- 自动分析 DAG 并渲染执行流程图，标注 parallel/serial 层级
- 执行中的步骤显示旋转动画 spinner + 实时计时
- 颜色区分状态：青色 (执行中) / 绿色 (成功) / 红色 (失败) / 黄色 (重试中)
- 所有步骤在同一流程图中原地刷新，不产生多行滚动

```
  ╔═══ Workflow: my-workflow (8 steps) ═══╗

  ┌─ Layer 1 ── parallel (3) ─────────────────────
  │  ✓ 获取数据 A [http] (0.8s)
  │  ✓ 获取数据 B [http] (1.2s)
  │  ⠹ 获取数据 C [http] 2.1s
  └─────────────────────────────────────────────
                 │
                 ▼
  ┌─ Layer 2 ── parallel (3) ─────────────────────
  │  ⠼ 分析 A [ollama/qwen3:8b] 12.4s
  │  ○ 分析 B [ollama/qwen3:8b]
  │  ○ 分析 C [ollama/qwen3:8b]
  └─────────────────────────────────────────────
```

### WASM 插件系统 (`rustflow-plugins`)

使用任何能编译到 WebAssembly 的语言编写自定义工具，扩展 RustFlow 的能力。

**架构：**
- 插件编译为 `.wasm`，通过 `wasmtime` 在运行时 JIT 加载，运行在安全沙箱中
- 每个导出的工具自动成为实现 `Tool` trait 的 `PluginTool`
- 插件工具与内置工具注册到同一个 `ToolRegistry`，对工作流完全透明

**插件 ABI**（插件必须导出的接口）：

| 导出名 | 签名 | 说明 |
|--------|------|------|
| `memory` | 线性内存 | 共享线性内存 |
| `rustflow_alloc` | `(size: i32) -> i32` | 分配内存，返回指针 |
| `rustflow_dealloc` | `(ptr: i32, size: i32)` | 释放内存 |
| `rustflow_plugin_manifest` | `() -> i64` | 打包的 `(ptr << 32 \| len)` 指向 manifest JSON |
| `rustflow_tool_execute` | `(name_ptr, name_len, input_ptr, input_len: i32) -> i64` | 执行工具，返回打包的输出 JSON 指针 |

插件还需导入 `rustflow::log(level: i32, ptr: i32, len: i32)` 用于宿主侧日志。

**用法：**
```rust
let mut loader = PluginLoader::new();
let tools = loader.load_file("plugins/my-plugin.wasm")?;
for tool in tools {
    tool_registry.register(tool).ok();
}
```

### 安全沙箱 (`SecurityPolicy`)

RustFlow 在所有工具执行过程中实施可配置的安全策略，防止路径穿越、命令注入和凭据泄露。

**文件系统沙箱** (`file_read` / `file_write`)：
- 路径规范化 — 访问前解析 `..` 和相对路径
- 允许目录白名单 — 将文件访问限制在指定目录内
- 符号链接拒绝 — 默认阻止符号链接以防止逃逸
- 敏感路径黑名单 — `.ssh`、`.env`、`/etc/shadow` 等始终被拒绝
- 写入大小限制 — 防止内存/磁盘耗尽（默认 50 MB）

**Shell 沙箱** (`shell`)：
- 默认禁用 — 必须在策略中显式启用
- 命令白名单 — 仅允许执行已批准的命令
- 危险环境变量过滤 — `LD_PRELOAD`、`DYLD_INSERT_LIBRARIES` 等会被过滤
- 输出截断 — 防止无限输出导致内存耗尽（默认 1 MB）
- 超时钳制 — 无论步骤配置如何，强制执行最大超时（默认 300 秒）

**环境变量保护** (`env`)：
- 禁止批量读取 — 默认禁止一次读取所有环境变量
- 敏感值脱敏 — 匹配 `*KEY*`、`*SECRET*`、`*PASSWORD*`、`*TOKEN*` 等模式的变量值返回 `[REDACTED]`

策略按执行配置，在构造工具时传入。完整配置选项参见 `rustflow-tools` 中的 `SecurityPolicy`。

### 熔断器 (`rustflow-core`)

RustFlow 的熔断器保护 LLM 提供商和工具免受级联故障影响。熔断器直接集成在调度器中，无需修改工作流 YAML。

**状态机：**

```
Closed   ──(连续失败达 failure_threshold)──► Open
Open     ──(timeout_ms 超时后)────────────► HalfOpen
HalfOpen ──(连续成功达 success_threshold)──► Closed
HalfOpen ──(任意失败)──────────────────────► Open
```

**按资源隔离：** 每个 LLM 提供商名称（如 `openai`、`ollama`）和工具名称（如 `http`、`shell`）拥有独立的熔断器。

**配置：**

```rust
CircuitBreakerConfig {
    failure_threshold: 5,   // 触发熔断的连续失败次数
    success_threshold: 2,   // HalfOpen 状态下恢复所需的连续成功次数
    timeout_ms: 30_000,     // Open 状态持续时间 (ms)，超时后进入 HalfOpen
}
```

**用法：**

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

**触发事件：**

| 事件 | 触发时机 |
|------|---------|
| `CircuitBreakerOpened { resource }` | 熔断器转入 Open 状态（或 Open 状态拦截步骤） |
| `CircuitBreakerClosed { resource }` | 熔断器从 HalfOpen 恢复为 Closed |

### HTTP Server (`rustflow-server`)

| 端点 | 方法 | 说明 |
|------|------|------|
| `/health` | GET | 健康检查 |
| `/agents` | POST | 创建 Agent |
| `/agents` | GET | 列出所有 Agent |
| `/agents/{id}` | GET | 获取 Agent 详情 |
| `/agents/{id}` | DELETE | 删除 Agent |
| `/agents/{id}/run` | POST | 执行 Agent 工作流（同步，返回最终输出） |
| `/agents/{id}/stream` | **WS** | 执行 Agent 并实时流式推送事件 |
| `/playground` | GET | Web Playground 交互界面 |
| `/playground/agents` | POST | 从 YAML 创建 Agent（Playground 专用） |

### WebSocket 实时流式输出（`/agents/{id}/stream`）

通过 WebSocket 连接，实时接收执行过程中的 JSON 事件帧。

**客户端 → 服务端（一条启动消息）：**
```json
{ "vars": { "topic": "Rust", "lang": "中文" } }
```

**服务端 → 客户端（流式事件帧）：**
```json
{"type":"step_started",   "step_id":"fetch","step_name":"获取数据"}
{"type":"step_succeeded", "step_id":"fetch","step_name":"获取数据","elapsed_ms":820,"output":{…}}
{"type":"step_failed",    "step_id":"s2","error":"…","will_retry":true,"attempt":1,"elapsed_ms":12}
{"type":"step_retrying",  "step_id":"s2","attempt":2}
{"type":"circuit_breaker_opened","resource":"ollama"}
{"type":"circuit_breaker_closed","resource":"ollama"}
{"type":"workflow_completed","outputs":{"fetch":{…},"summarise":"…"}}
{"type":"workflow_failed","error":"step 'fetch' failed after all retries"}
```

`workflow_completed` 或 `workflow_failed` 消息发送后，服务端主动关闭连接。

## 快速开始

### 安装

```bash
# 从源码构建
git clone https://github.com/rust-ai-flow/rustflow.git
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
| 终端 UI | crossterm (颜色、光标控制、spinner 动画) |
| 可观测性 | tracing + tracing-subscriber |
| CLI 解析 | clap (derive) |
| 错误处理 | thiserror + anyhow |
| 插件沙箱 | wasmtime (规划中) |
| 测试 | tokio::test, 367 个单元测试 |

## 开发

```bash
cargo build                          # 构建所有 crate
cargo test                           # 运行所有测试 (367 个)
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
- [x] 异步并发调度器（依赖驱动 + 重试 + 事件回调）
- [x] LLM 多 Provider 网关 (OpenAI, Anthropic, Ollama)
- [x] Tool 系统 + 7 个内置工具 (http, file_read, file_write, shell, json_extract, env, sleep)
- [x] YAML 工作流定义与加载
- [x] HTTP API 服务器 (Agent CRUD + 执行)
- [x] CLI 工具 (run, init, serve, doctor)
- [x] 实时流程图进度显示（DAG 可视化、spinner 动画、颜色状态）
- [x] 安全沙箱（文件系统隔离、Shell 白名单、环境变量脱敏）
- [x] WASM 插件系统 (wasmtime)
- [x] 熔断器（Closed/Open/HalfOpen 状态机，按资源隔离，调度器集成）
- [x] WebSocket 实时流式输出（`/agents/{id}/stream`，逐步事件帧 + 最终输出）
- [x] Web Playground 交互界面（单文件 HTML + React/TS 源码，嵌入 `/playground`）
- [ ] Python SDK (PyO3)
- [ ] TypeScript SDK
- [ ] Prometheus 指标 + OpenTelemetry
- [ ] Tauri 桌面应用

## 许可

Apache-2.0 (核心) / BSL 1.1 (企业功能)
