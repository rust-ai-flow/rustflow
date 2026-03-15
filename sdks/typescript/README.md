# rustflow

TypeScript / JavaScript SDK for [RustFlow](https://github.com/rust-ai-flow/rustflow) — a high-performance AI Agent orchestration runtime built in Rust.

## Installation

```bash
npm install rustflow
# or
pnpm add rustflow
# or
yarn add rustflow
```

## Requirements

- Node.js ≥ 18 (native `fetch`)
- A running RustFlow server (`rustflow serve`, default port `18790`)

## Quick Start

```ts
import { RustFlowClient, toolStep, llmStep } from "rustflow";

const client = new RustFlowClient({ baseUrl: "http://localhost:18790" });

// Create an agent from step definitions
const { id } = await client.createAgent({
  name: "summarise-workflow",
  steps: [
    toolStep("fetch", "Fetch Data", "http", {
      url: "https://httpbin.org/json",
      method: "GET",
    }),
    llmStep(
      "summarise",
      "Summarise",
      { provider: "ollama", model: "llama3", prompt: "Summarise: {{steps.fetch.output}}" },
      { depends_on: ["fetch"] },
    ),
  ],
});

// Run and stream events
for await (const event of client.stream(id)) {
  switch (event.type) {
    case "step_started":
      console.log(`▶ ${event.step_name}`);
      break;
    case "step_succeeded":
      console.log(`✓ ${event.step_name} (${event.elapsed_ms}ms)`);
      break;
    case "step_failed":
      console.error(`✗ ${event.step_name}: ${event.error}`);
      break;
    case "workflow_completed":
      console.log("outputs:", event.outputs);
      break;
  }
}
```

## API Reference

### `new RustFlowClient(options?)`

| Option | Type | Default |
|--------|------|---------|
| `baseUrl` | `string` | `"http://localhost:18790"` |

---

### Agent CRUD

#### `client.health()`
```ts
const { status, version } = await client.health();
```

#### `client.createAgent(req)`
```ts
const { id } = await client.createAgent({
  name: "my-agent",
  description: "optional",   // optional
  steps: [ /* Step[] */ ],
});
```

#### `client.listAgents()`
```ts
const { agents, count } = await client.listAgents();
// agents: AgentSummary[]
```

#### `client.getAgent(id)`
```ts
const agent = await client.getAgent("agent-id");
// agent: Agent  (includes full steps array)
```

#### `client.deleteAgent(id)`
```ts
await client.deleteAgent("agent-id");
```

---

### Execution

#### `client.runAgent(id, vars?)`

Blocking — waits for the entire workflow to finish.

```ts
const result = await client.runAgent("agent-id", { topic: "Rust", lang: "en" });
// result.status   → "completed"
// result.outputs  → { stepId: outputValue, ... }
```

#### `client.stream(id, options?)` → `AsyncGenerator<WsEvent>`

Streams execution events via WebSocket. The generator yields each event as it arrives and returns the terminal event (`workflow_completed` or `workflow_failed`).

```ts
const terminal = await (async () => {
  let last;
  for await (const event of client.stream("agent-id", { vars: { topic: "Rust" } })) {
    last = event;
  }
  return last;
})();
```

`StreamOptions`:

| Option | Type | Description |
|--------|------|-------------|
| `vars` | `Record<string, unknown>` | Input variables |
| `onEvent` | `(event: WsEvent) => void` | Called for every event (alternative to the generator) |

---

### Playground

#### `client.createFromYaml(yaml)`

Parse a YAML workflow definition and register it as an agent.

```ts
const yaml = `
name: hello-agent
steps:
  - id: fetch
    name: Fetch Data
    tool:
      name: http
      input:
        url: https://httpbin.org/json
        method: GET
`;

const { id } = await client.createFromYaml(yaml);
const result = await client.runAgent(id);
```

---

### Step Builder Helpers

#### `toolStep(id, name, tool, input, options?)`

```ts
toolStep("fetch", "Fetch Data", "http", {
  url: "https://api.example.com/data",
  method: "GET",
  headers: { "Authorization": "Bearer {{vars.token}}" },
})
```

#### `llmStep(id, name, config, options?)`

```ts
llmStep("summarise", "Summarise", {
  provider: "openai",      // "openai" | "anthropic" | "ollama"
  model: "gpt-4o",
  prompt: "Summarise: {{steps.fetch.output}}",
  temperature: 0.7,        // optional
  max_tokens: 500,         // optional
})
```

`StepBuilderOptions` (both builders):

| Option | Type | Description |
|--------|------|-------------|
| `depends_on` | `string[]` | Step IDs that must complete first |
| `retry_policy` | `RetryPolicy` | Retry strategy |
| `timeout_ms` | `number` | Per-step timeout |

---

### Retry Policies

```ts
// No retry (default)
{ kind: "none" }

// Fixed interval
{ kind: "fixed", max_retries: 3, interval_ms: 1000 }

// Exponential backoff
{
  kind: "exponential",
  max_retries: 5,
  initial_interval_ms: 500,
  multiplier: 2,
  max_interval_ms: 30_000,
}
```

---

### WebSocket Events

All events have a discriminated `type` field for exhaustive narrowing:

| `type` | Extra fields |
|--------|-------------|
| `step_started` | `step_id`, `step_name` |
| `step_succeeded` | `step_id`, `step_name`, `elapsed_ms`, `output` |
| `step_failed` | `step_id`, `step_name`, `error`, `will_retry`, `attempt`, `elapsed_ms` |
| `step_retrying` | `step_id`, `step_name`, `attempt` |
| `circuit_breaker_opened` | `resource` |
| `circuit_breaker_closed` | `resource` |
| `workflow_completed` | `outputs` (`Record<string, unknown>`) |
| `workflow_failed` | `error` |

---

### Error Handling

All HTTP errors throw a `RustFlowError`:

```ts
import { RustFlowClient, RustFlowError } from "rustflow";

try {
  await client.getAgent("nonexistent");
} catch (e) {
  if (e instanceof RustFlowError) {
    console.error(e.message);  // "agent 'nonexistent' not found"
    console.error(e.status);   // 404
  }
}
```

---

## Complete Example

```ts
import { RustFlowClient, toolStep, llmStep } from "rustflow";

const client = new RustFlowClient();

async function main() {
  // 1. Create agent
  const { id } = await client.createAgent({
    name: "research-agent",
    steps: [
      toolStep("fetch", "Fetch Page", "http", {
        url: "https://httpbin.org/json",
        method: "GET",
      }),
      toolStep("save", "Save Raw", "file_write", {
        path: "./output/raw.json",
        content: "{{steps.fetch.output}}",
      }, { depends_on: ["fetch"] }),
      llmStep("analyse", "Analyse", {
        provider: "ollama",
        model: "qwen3:8b",
        prompt: "Analyse this JSON and give a one-paragraph summary: {{steps.fetch.output}}",
        max_tokens: 300,
      }, {
        depends_on: ["fetch"],
        retry_policy: { kind: "fixed", max_retries: 2, interval_ms: 2000 },
      }),
    ],
  });

  console.log(`Agent created: ${id}`);

  // 2. Stream execution
  for await (const event of client.stream(id)) {
    if (event.type === "step_started")   console.log(`  ▶ ${event.step_name}`);
    if (event.type === "step_succeeded") console.log(`  ✓ ${event.step_name} (${event.elapsed_ms}ms)`);
    if (event.type === "step_failed")    console.error(`  ✗ ${event.step_name}: ${event.error}`);
    if (event.type === "workflow_completed") {
      console.log("\nAnalysis:", event.outputs["analyse"]);
    }
    if (event.type === "workflow_failed") {
      throw new Error(event.error);
    }
  }

  // 3. Cleanup
  await client.deleteAgent(id);
}

main().catch(console.error);
```

## LLM Providers

| Provider | `provider` value | Auth |
|----------|-----------------|------|
| OpenAI | `"openai"` | `OPENAI_API_KEY` env var |
| Anthropic | `"anthropic"` | `ANTHROPIC_API_KEY` env var |
| Ollama | `"ollama"` | None (local, `localhost:11434`) |

## License

Apache-2.0
