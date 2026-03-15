import { afterEach, describe, expect, jest, test } from "@jest/globals";
import { RustFlowClient, RustFlowError, llmStep, toolStep } from "./client.js";
import type { WsEvent } from "./types.js";

// ── Fetch mock helpers ────────────────────────────────────────────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyFn = (...args: any[]) => any;

function mockFetch(status: number, body: unknown): void {
  // Cast through `any` to avoid fighting the overloaded fetch signature.
  (global as Record<string, unknown>)["fetch"] = jest.fn<AnyFn>().mockResolvedValueOnce({
    ok: status >= 200 && status < 300,
    status,
    statusText: status === 200 ? "OK" : "Error",
    json: async () => body,
    text: async () => JSON.stringify(body),
  });
}

function fetchMock(): jest.MockedFunction<AnyFn> {
  return global.fetch as jest.MockedFunction<AnyFn>;
}

// ── RustFlowClient: HTTP methods ──────────────────────────────────────────────

describe("RustFlowClient", () => {
  const client = new RustFlowClient({ baseUrl: "http://localhost:18790" });

  afterEach(() => { jest.resetAllMocks(); });

  // ── health ────────────────────────────────────────────────────────────────

  test("health() returns status and version", async () => {
    mockFetch(200, { status: "ok", version: "0.1.0" });
    const res = await client.health();
    expect(res.status).toBe("ok");
    expect(res.version).toBe("0.1.0");
  });

  // ── createAgent ───────────────────────────────────────────────────────────

  test("createAgent() posts to /agents and returns id", async () => {
    mockFetch(201, { id: "abc-123", message: "agent created" });

    const res = await client.createAgent({
      name: "test-agent",
      steps: [
        toolStep("fetch", "Fetch", "http", { url: "https://example.com", method: "GET" }),
      ],
    });

    expect(res.id).toBe("abc-123");
    expect(fetchMock().mock.calls[0][0]).toContain("/agents");
    const init = fetchMock().mock.calls[0][1] as RequestInit;
    expect(init.method).toBe("POST");
    const body = JSON.parse(init.body as string);
    expect(body.name).toBe("test-agent");
    expect(body.steps).toHaveLength(1);
  });

  // ── listAgents ────────────────────────────────────────────────────────────

  test("listAgents() returns agents array and count", async () => {
    mockFetch(200, {
      agents: [{ id: "1", name: "a", step_count: 2, created_at: "2026-01-01T00:00:00Z" }],
      count: 1,
    });
    const res = await client.listAgents();
    expect(res.count).toBe(1);
    expect(res.agents[0].name).toBe("a");
  });

  // ── getAgent ──────────────────────────────────────────────────────────────

  test("getAgent() fetches by id", async () => {
    const agentPayload = {
      id: "abc-123",
      name: "my-agent",
      step_count: 1,
      created_at: "2026-01-01T00:00:00Z",
      steps: [],
    };
    mockFetch(200, agentPayload);
    const agent = await client.getAgent("abc-123");
    expect(agent.id).toBe("abc-123");
    const url = fetchMock().mock.calls[0][0] as string;
    expect(url).toContain("/agents/abc-123");
  });

  // ── deleteAgent ───────────────────────────────────────────────────────────

  test("deleteAgent() sends DELETE request", async () => {
    mockFetch(200, { message: "agent 'abc-123' deleted" });
    const res = await client.deleteAgent("abc-123");
    expect(res.message).toContain("deleted");
    const init = fetchMock().mock.calls[0][1] as RequestInit;
    expect(init.method).toBe("DELETE");
  });

  // ── runAgent ──────────────────────────────────────────────────────────────

  test("runAgent() sends vars and returns outputs", async () => {
    mockFetch(200, { agent_id: "abc-123", status: "completed", outputs: { fetch: "data" } });
    const res = await client.runAgent("abc-123", { topic: "Rust" });
    expect(res.status).toBe("completed");
    expect(res.outputs.fetch).toBe("data");
    const init = fetchMock().mock.calls[0][1] as RequestInit;
    const body = JSON.parse(init.body as string);
    expect(body.vars.topic).toBe("Rust");
  });

  // ── createFromYaml ────────────────────────────────────────────────────────

  test("createFromYaml() posts yaml and returns id", async () => {
    mockFetch(201, { id: "yaml-agent", message: "agent created" });
    const res = await client.createFromYaml("name: test\nsteps: []");
    expect(res.id).toBe("yaml-agent");
    const url = fetchMock().mock.calls[0][0] as string;
    expect(url).toContain("/playground/agents");
    const body = JSON.parse(fetchMock().mock.calls[0][1].body as string);
    expect(body.yaml).toContain("name: test");
  });

  // ── error handling ────────────────────────────────────────────────────────

  test("throws RustFlowError on 404", async () => {
    mockFetch(404, { error: "agent 'x' not found" });
    const err = await client.getAgent("x").catch((e: unknown) => e);
    expect(err).toBeInstanceOf(RustFlowError);
    expect((err as RustFlowError).message).toBe("agent 'x' not found");
    expect((err as RustFlowError).status).toBe(404);
  });

  test("throws RustFlowError on 500 with status code", async () => {
    mockFetch(500, { error: "internal error" });
    try {
      await client.health();
    } catch (e) {
      expect(e).toBeInstanceOf(RustFlowError);
      expect((e as RustFlowError).status).toBe(500);
    }
  });

  // ── baseUrl normalisation ─────────────────────────────────────────────────

  test("strips trailing slash from baseUrl", async () => {
    const c = new RustFlowClient({ baseUrl: "http://localhost:18790/" });
    mockFetch(200, { status: "ok", version: "0.1.0" });
    await c.health();
    const url = fetchMock().mock.calls[0][0] as string;
    expect(url).toBe("http://localhost:18790/health");
  });

  test("defaults to localhost:18790", async () => {
    const c = new RustFlowClient();
    mockFetch(200, { status: "ok", version: "0.1.0" });
    await c.health();
    const url = fetchMock().mock.calls[0][0] as string;
    expect(url).toContain("localhost:18790");
  });
});

// ── Step builder helpers ───────────────────────────────────────────────────────

describe("toolStep()", () => {
  test("builds a tool step with defaults", () => {
    const step = toolStep("fetch", "Fetch", "http", { url: "https://example.com" });
    expect(step.id).toBe("fetch");
    expect(step.name).toBe("Fetch");
    expect(step.kind).toEqual({ tool: { tool: "http", input: { url: "https://example.com" } } });
    expect(step.depends_on).toEqual([]);
    expect(step.retry_policy).toEqual({ kind: "none" });
    expect(step.timeout_ms).toBeUndefined();
  });

  test("builds a tool step with all options", () => {
    const step = toolStep("write", "Write", "file_write", { path: "/tmp/out.json" }, {
      depends_on: ["fetch"],
      retry_policy: { kind: "fixed", max_retries: 3, interval_ms: 1000 },
      timeout_ms: 5000,
    });
    expect(step.depends_on).toEqual(["fetch"]);
    expect(step.retry_policy).toEqual({ kind: "fixed", max_retries: 3, interval_ms: 1000 });
    expect(step.timeout_ms).toBe(5000);
  });
});

describe("llmStep()", () => {
  test("builds an llm step with defaults", () => {
    const step = llmStep("summarise", "Summarise", {
      provider: "ollama",
      model: "llama3",
      prompt: "Summarise: {{steps.fetch.output}}",
    });
    expect(step.id).toBe("summarise");
    expect(step.kind).toEqual({
      llm: { provider: "ollama", model: "llama3", prompt: "Summarise: {{steps.fetch.output}}" },
    });
    expect(step.depends_on).toEqual([]);
  });

  test("builds an llm step with depends_on and exponential retry", () => {
    const step = llmStep(
      "analyze",
      "Analyze",
      { provider: "openai", model: "gpt-4o", prompt: "Analyze this" },
      {
        depends_on: ["fetch", "save"],
        retry_policy: {
          kind: "exponential",
          max_retries: 5,
          initial_interval_ms: 500,
          multiplier: 2,
          max_interval_ms: 30000,
        },
      },
    );
    expect(step.depends_on).toEqual(["fetch", "save"]);
    const rp = step.retry_policy;
    expect(rp.kind).toBe("exponential");
    if (rp.kind === "exponential") {
      expect(rp.max_retries).toBe(5);
      expect(rp.multiplier).toBe(2);
    }
  });
});

// ── WsEvent type narrowing ────────────────────────────────────────────────────

describe("WsEvent type narrowing", () => {
  test("step_started narrows correctly", () => {
    const e: WsEvent = { type: "step_started", step_id: "s1", step_name: "S1" };
    expect(e.type).toBe("step_started");
    if (e.type === "step_started") {
      expect(e.step_id).toBe("s1");
    }
  });

  test("step_succeeded narrows with output and elapsed_ms", () => {
    const e: WsEvent = {
      type: "step_succeeded",
      step_id: "s1",
      step_name: "S1",
      elapsed_ms: 820,
      output: { result: "ok" },
    };
    if (e.type === "step_succeeded") {
      expect(e.elapsed_ms).toBe(820);
      expect(e.output).toEqual({ result: "ok" });
    }
  });

  test("workflow_completed narrows with outputs", () => {
    const e: WsEvent = {
      type: "workflow_completed",
      outputs: { s1: "result" },
    };
    if (e.type === "workflow_completed") {
      expect(e.outputs.s1).toBe("result");
    }
  });

  test("circuit_breaker_opened has resource field", () => {
    const e: WsEvent = { type: "circuit_breaker_opened", resource: "ollama" };
    if (e.type === "circuit_breaker_opened") {
      expect(e.resource).toBe("ollama");
    }
  });
});

// ── RustFlowError ─────────────────────────────────────────────────────────────

describe("RustFlowError", () => {
  test("is an instance of Error", () => {
    const err = new RustFlowError("boom", 500, { error: "boom" });
    expect(err).toBeInstanceOf(Error);
    expect(err).toBeInstanceOf(RustFlowError);
    expect(err.name).toBe("RustFlowError");
    expect(err.message).toBe("boom");
    expect(err.status).toBe(500);
    expect(err.body).toEqual({ error: "boom" });
  });

  test("works without optional params", () => {
    const err = new RustFlowError("network error");
    expect(err.status).toBeUndefined();
    expect(err.body).toBeUndefined();
  });
});
