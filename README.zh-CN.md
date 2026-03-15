<p align="center">
  <img src="./logo/rustflow-logo-full.svg" alt="RustFlow Logo" width="300">
</p>

<h1 align="center">RustFlow</h1>

<p align="center">
  高性能 AI Agent 编排运行时，用 Rust 重写 Agent 基础设施。
</p>

<p align="center">
  <a href="./README.md">English</a> ·
  <a href="#快速开始">快速开始</a> ·
  <a href="#web-playground">Playground</a> ·
  <a href="#http-api">API</a> ·
  <a href="#typescript-sdk">TypeScript SDK</a> ·
  <a href="#架构概览">架构</a>
</p>

---

## 为什么选择 RustFlow

Python Agent 框架（LangChain、AutoGen）每个 Agent 占用 **100–300 MB** 内存，并发上限低。RustFlow 用 Rust 从底层重写编排层：

| | Python (LangChain) | RustFlow |
|---|---|---|
| 内存 / Agent | ~200 MB | **~3 MB** |
| 并发 Agent 数 | ~100 | **10,000+** |
| 冷启动时间 | 2–5 秒 | **< 50 毫秒** |
| 部署产物 | 500 MB Docker 镜像 | **5 MB 单文件** |
| 基础设施成本（1k Agent） | ~$2,000 / 月 | **~$500 / 月** |

## 性能基准

以下数据在 release 构建下实测（`cargo bench` / `cargo test --release`），Apple M 系列芯片，tokio 多线程运行时。

### 并发吞吐 — 调度器

| 场景 | Agent 数 | 步骤数 / Agent | 耗时 |
|---|---|---|---|
| 单步 Agent | 10,000 | 1 | **48 ms** |
| 并行工作流 | 1,000 | 10（全部独立） | **36 ms** |
| 串行链 | 500 | 20（顺序依赖） | **25 ms** |

### DAG 解析延迟（µs，中位数）

| 拓扑 | 100 步 | 1,000 步 | 5,000 步 |
|---|---|---|---|
| 串行链 | ~8 µs | ~90 µs | ~490 µs |
| 全并行 | ~5 µs | ~55 µs | ~290 µs |
| 菱形（N 条分支） | ~9 µs | — | — |

### 核心类型操作（ns，中位数）

| 操作 | 10 项 | 100 项 | 1,000 项 |
|---|---|---|---|
| `Agent::new` | ~800 ns | ~7 µs | ~70 µs |
| Agent 序列化 | ~1 µs | ~10 µs | ~100 µs |
| Agent 反序列化 | ~2 µs | ~18 µs | ~180 µs |
| `Context::set_step_output` × N | ~600 ns | ~5 µs | ~55 µs |

本地复现：

```bash
# 微基准（HTML 报告 → target/criterion/）
cargo bench -p rustflow-benches

# 并发压测
cargo test -p rustflow-benches --test concurrency --release -- --nocapture
```

---

## 快速开始

### 安装

```bash
curl -fsSL https://raw.githubusercontent.com/rust-ai-flow/rustflow/master/install.sh | bash
```

从源码编译，安装到 `~/.rustflow/bin`，自动更新 shell `PATH`。需要 Cargo（若未安装，脚本会提示安装 rustup）。

```bash
# 验证安装
rustflow --version
rustflow doctor --full
```

或手动构建：

```bash
git clone https://github.com/rust-ai-flow/rustflow.git
cd rustflow
cargo build --release -p rustflow-cli
# 二进制位于 target/release/rustflow
```

### 编写工作流

```yaml
# research.yaml
name: research-and-summarize
description: 抓取网页并用本地 LLM 总结

steps:
  - id: fetch
    name: 抓取网页
    tool:
      name: http
      input:
        url: "{{vars.url}}"
        method: GET

  - id: summarize
    name: AI 总结
    depends_on: [fetch]
    llm:
      provider: ollama
      model: llama3.2
      prompt: |
        用 3 条要点总结以下内容：
        {{steps.fetch.output.body}}
    retry:
      kind: exponential
      max_retries: 3
      initial_ms: 1000
```

### 运行

```bash
# 执行工作流
rustflow run research.yaml --vars '{"url":"https://example.com"}'

# 监听文件变更，保存即重跑
rustflow dev research.yaml --vars '{"url":"https://example.com"}'

# 启动 API 服务器
rustflow serve

# 打开 Web Playground
rustflow playground
```

---

## CLI

```
rustflow <command> [options]
```

| 命令 | 说明 |
|---|---|
| `run <file>` | 执行 YAML 工作流，实时显示进度 |
| `run <file> --watch` | 文件变更时自动重跑（`dev` 的别名） |
| `dev <file>` | 监听模式，保存即执行 |
| `serve` | 启动 HTTP + WebSocket API 服务器（端口 18790） |
| `playground` | 启动服务器并打开 Web Playground |
| `doctor` | 检查环境（Rust、Ollama、API Key、服务器） |
| `doctor --full` | 包含 Provider 和服务器连通性检查 |
| `init [name]` | 初始化项目，生成示例工作流 |

### `rustflow run` — 实时流程图

```
  ╔═══ Workflow: research-and-summarize (2 steps) ═══╗

  ┌─ Layer 1 ───────────────────────────────────────
  │  ✓ 抓取网页 [http] (342ms)
  └──────────────────────────────────────────────────
                   │
                   ▼
  ┌─ Layer 2 ───────────────────────────────────────
  │  ⠹ AI 总结 [ollama/llama3.2] 4.1s
  └──────────────────────────────────────────────────
```

所有步骤在同一流程图中原地刷新，不产生滚动噪音。

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
# 打开 http://localhost:5173/playground/
```

三栏 Web UI，用于可视化编写、运行和调试工作流。

**功能特性：**

- **YAML 编辑器** — 带语法高亮，直接在浏览器里写工作流
- **实时执行面板** — 每个步骤独立卡片，显示 spinner、耗时、输出
- **并发运行** — 同时运行多个工作流，切换查看各自状态，互不干扰
- **刷新不丢数据** — 执行历史保存在 localStorage；刷新页面后自动重连服务端仍在运行的任务
- **工作流侧边栏** — 列出所有已注册的 Agent；橙色脉冲点标记正在运行的任务
- **输入变量** — 在顶栏以 JSON 格式传入 `vars`，点击 Run 即可执行

---

## HTTP API

默认端口：**18790**

| 端点 | 方法 | 说明 |
|---|---|---|
| `/health` | GET | 健康检查 + 版本号 |
| `/agents` | POST | 从步骤列表创建 Agent |
| `/agents` | GET | 列出所有 Agent |
| `/agents/{id}` | GET | 获取 Agent 详情和步骤 |
| `/agents/{id}` | DELETE | 删除 Agent |
| `/agents/{id}/run` | POST | 同步执行，等待并返回最终结果 |
| `/agents/{id}/stream` | **WS** | 执行并实时流式推送事件 |
| `/agents/{id}/observe` | **WS** | 以只读方式接入已有的运行（重连） |
| `/playground/agents` | POST | 从 YAML 创建 Agent（Playground 专用） |

### WebSocket 流式协议

#### `/agents/{id}/stream` — 发起或接入运行

连接后发送启动消息，持续接收事件：

```json
// 客户端 → 服务端（连接后发送一次）
{ "vars": { "url": "https://example.com" } }

// 服务端 → 客户端（流式推送）
{"type":"step_started",   "step_id":"fetch","step_name":"抓取网页"}
{"type":"step_succeeded", "step_id":"fetch","elapsed_ms":342,"output":{…}}
{"type":"step_started",   "step_id":"summarize","step_name":"AI 总结"}
{"type":"step_retrying",  "step_id":"summarize","attempt":2}
{"type":"step_succeeded", "step_id":"summarize","elapsed_ms":4100,"output":"…"}
{"type":"workflow_completed","outputs":{"fetch":{…},"summarize":"…"}}
```

如果该 Agent 已有进行中的运行，新连接直接作为 observer 接入，**不会重复执行**。

#### `/agents/{id}/observe` — 重连已有运行

协议与 `/stream` 完全相同。先回放已产生的所有事件，再持续推送直到工作流结束。若服务端没有该 Agent 的活跃运行，立即返回 `workflow_failed`。

```
适用场景：页面刷新、网络断线后重连。
```

**事件类型速查：**

| 类型 | 字段 |
|---|---|
| `step_started` | `step_id`、`step_name` |
| `step_succeeded` | `step_id`、`step_name`、`elapsed_ms`、`output` |
| `step_failed` | `step_id`、`step_name`、`error`、`will_retry`、`attempt`、`elapsed_ms` |
| `step_retrying` | `step_id`、`step_name`、`attempt` |
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

// 从步骤列表创建 Agent
const { id } = await client.createAgent({
  name: 'my-workflow',
  steps: [
    toolStep('fetch', '抓取网页', 'http', { url: 'https://example.com', method: 'GET' }),
    llmStep('summarize', 'AI 总结', {
      provider: 'ollama',
      model: 'llama3.2',
      prompt: '总结：{{steps.fetch.output.body}}',
    }, { depends_on: ['fetch'] }),
  ],
});

// 流式执行，实时接收事件
for await (const event of client.stream(id, { vars: {} })) {
  if (event.type === 'step_succeeded') {
    console.log(event.step_name, event.output);
  }
}

// 重连进行中的任务（如页面刷新后）
for await (const event of client.observe(id)) {
  console.log(event.type);
}
```

### API 速查

| 方法 | 说明 |
|---|---|
| `health()` | `GET /health` |
| `createAgent(req)` | `POST /agents` |
| `listAgents()` | `GET /agents` |
| `getAgent(id)` | `GET /agents/:id` |
| `deleteAgent(id)` | `DELETE /agents/:id` |
| `runAgent(id, vars?)` | `POST /agents/:id/run` — 同步等待 |
| `stream(id, options?)` | `WS /agents/:id/stream` — 异步生成器 |
| `observe(id, options?)` | `WS /agents/:id/observe` — 异步生成器，用于重连 |
| `createFromYaml(yaml)` | `POST /playground/agents` |

`stream()` 和 `observe()` 均返回 `AsyncGenerator<WsEvent, WorkflowCompletedEvent | WorkflowFailedEvent>`。

---

## 架构概览

```
┌──────────────────────────────────────────────────────────┐
│                        API 层                             │
│   HTTP/REST (axum)  ·  WebSocket 流式  ·  多语言 SDK      │
├──────────────────────────────────────────────────────────┤
│                      编排层                               │
│   DAG 解析器  ·  异步调度器  ·  事件广播                    │
├──────────────────────────────────────────────────────────┤
│                      执行层                               │
│   LLM 网关  ·  Tool 引擎  ·  重试 / 熔断器                 │
├──────────────────────────────────────────────────────────┤
│                      基础层                               │
│   tokio  ·  tracing  ·  serde  ·  wasmtime (WASM)        │
└──────────────────────────────────────────────────────────┘
```

### Workspace 结构

```
crates/
  rustflow-core/          Agent、Step、Context、RetryPolicy、CircuitBreaker
  rustflow-orchestrator/  DAG 解析器、异步调度器、事件回调
  rustflow-llm/           LLM 网关：OpenAI、Anthropic、Ollama
  rustflow-tools/         Tool trait + 7 个内置工具
  rustflow-plugins/       WASM 插件加载器（wasmtime 沙箱）
  rustflow-server/        axum HTTP + WebSocket 服务器、运行状态存储
  rustflow-cli/           CLI 二进制
apps/
  playground/             React + TypeScript + Tailwind Web UI
sdks/
  typescript/             TypeScript SDK（npm: rustflow）
  python/                 Python 绑定（PyO3，规划中）
```

---

## 功能详情

### 内置工具

| 工具 | 注册名 | 说明 |
|---|---|---|
| `HttpTool` | `http` | HTTP 请求，支持 GET/POST/PUT/DELETE/PATCH，自定义 Header |
| `FileReadTool` | `file_read` | 读取文件，支持 UTF-8 和 base64 |
| `FileWriteTool` | `file_write` | 写入文件，支持覆盖/追加，自动创建目录 |
| `ShellTool` | `shell` | 执行 Shell 命令，返回 stdout/stderr/exit_code |
| `JsonExtractTool` | `json_extract` | 用点路径从 JSON 提取值，如 `users.0.name` |
| `EnvTool` | `env` | 读取环境变量，支持默认值 |
| `SleepTool` | `sleep` | 暂停执行（速率限制、退避等待） |

### 重试策略

```yaml
retry:
  kind: none           # 不重试（默认）

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

### 熔断器

保护 LLM Provider 和工具免受级联故障影响，按资源独立隔离。

```
Closed   ──(连续失败 N 次)──► Open
Open     ──(超时后)──────────► HalfOpen
HalfOpen ──(连续成功 M 次)──► Closed
HalfOpen ──(任意失败)────────► Open
```

发出事件：`circuit_breaker_opened`、`circuit_breaker_closed`。

### 安全沙箱

- **文件系统** — 路径规范化、目录白名单、拒绝符号链接、敏感路径黑名单（`.ssh`、`.env`、`/etc/shadow`）
- **Shell** — 默认禁用；命令白名单；过滤危险环境变量（`LD_PRELOAD` 等）
- **环境变量** — 禁止批量读取；`*KEY*`、`*SECRET*`、`*TOKEN*`、`*PASSWORD*` 返回 `[REDACTED]`

### WASM 插件系统

用任何能编译到 WebAssembly 的语言编写自定义工具：

```rust
let mut loader = PluginLoader::new();
let tools = loader.load_file("plugins/my-plugin.wasm")?;
for tool in tools {
    tool_registry.register(tool).ok();
}
```

---

## LLM Provider 配置

```bash
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
# Ollama 默认连接 localhost:11434，无需配置
```

---

## 开发

```bash
cargo build                     # 构建所有 crate
cargo test                      # 运行所有测试
cargo clippy --all-targets      # Lint 检查
cargo fmt --all -- --check      # 格式检查
cargo run -- doctor --full      # 检查开发环境
cargo run -- serve              # 启动服务器（:18790）
cargo run -- playground         # 启动服务器 + 打开 Playground
```

---

## 路线图

- [x] 核心类型系统（Agent、Step、Context、RetryPolicy）
- [x] DAG 解析与拓扑排序，检测环路
- [x] 异步并发调度器（依赖驱动 + 重试 + 事件回调）
- [x] LLM 多 Provider 网关（OpenAI、Anthropic、Ollama）
- [x] 7 个内置工具（http、file_read、file_write、shell、json_extract、env、sleep）
- [x] YAML 工作流定义与加载
- [x] HTTP API 服务器（Agent CRUD + 执行）
- [x] WebSocket 实时流式输出（`/agents/{id}/stream`）
- [x] WebSocket 重连观察（`/agents/{id}/observe`）
- [x] CLI — run、serve、doctor、dev、playground、init
- [x] 实时流程图进度（DAG 可视化、spinner 动画、颜色状态）
- [x] 安全沙箱（文件系统隔离、Shell 白名单、环境变量脱敏）
- [x] 熔断器（按资源隔离，调度器集成）
- [x] WASM 插件系统（wasmtime）
- [x] TypeScript SDK（`npm install rustflow`），含 `stream()` 和 `observe()`
- [x] Web Playground（并发运行、localStorage 持久化、自动重连）
- [x] 一键安装脚本（`curl … | bash`）
- [ ] Prometheus 指标 + OpenTelemetry
- [ ] Python SDK（PyO3）
- [ ] Tauri 桌面应用

---

## 许可

Apache-2.0（核心）/ BSL 1.1（企业功能）
