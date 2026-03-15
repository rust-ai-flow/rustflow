import type {
  Agent,
  AgentSummary,
  CreateAgentRequest,
  CreateAgentResponse,
  DeleteResponse,
  HealthResponse,
  ListAgentsResponse,
  LlmConfig,
  RunResult,
  Step,
  StepBuilderOptions,
  StepKind,
  ToolConfig,
  WsEvent,
  WorkflowCompletedEvent,
  WorkflowFailedEvent,
} from "./types.js";

// ── Error ─────────────────────────────────────────────────────────────────────

export class RustFlowError extends Error {
  constructor(
    message: string,
    public readonly status?: number,
    public readonly body?: unknown,
  ) {
    super(message);
    this.name = "RustFlowError";
  }
}

// ── Client options ────────────────────────────────────────────────────────────

export interface ClientOptions {
  /**
   * Base URL of the RustFlow server.
   * @default "http://localhost:18790"
   */
  baseUrl?: string;
  /**
   * WebSocket base URL of the RustFlow server.
   * If not provided, it will be derived from baseUrl by replacing http(s) with ws(s).
   */
  wsBaseUrl?: string;
}

export interface StreamOptions {
  /** Input variables to inject into the execution context. */
  vars?: Record<string, unknown>;
  /**
   * Called for every event received from the server (including terminal events).
   * The async generator also yields every event — use whichever fits your style.
   */
  onEvent?: (event: WsEvent) => void;
}

// ── Main client ───────────────────────────────────────────────────────────────

export class RustFlowClient {
  private readonly baseUrl: string;
  private readonly wsBaseUrl: string;

  constructor(options: ClientOptions = {}) {
    const base = (options.baseUrl ?? "http://localhost:18790").replace(/\/$/, "");
    this.baseUrl = base;
    // Use provided wsBaseUrl or derive from baseUrl
    if (options.wsBaseUrl) {
      this.wsBaseUrl = options.wsBaseUrl.replace(/\/$/, "");
    } else {
      // Convert http(s):// → ws(s)://
      this.wsBaseUrl = base.replace(/^http/, "ws");
    }
  }

  // ── Internal helpers ────────────────────────────────────────────────────────

  private async request<T>(
    path: string,
    init: RequestInit = {},
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    const response = await fetch(url, {
      headers: { "Content-Type": "application/json", ...init.headers },
      ...init,
    });

    if (!response.ok) {
      let body: unknown;
      try {
        body = await response.json();
      } catch {
        body = await response.text();
      }
      const message =
        typeof body === "object" && body !== null && "error" in body
          ? String((body as Record<string, unknown>).error)
          : `HTTP ${response.status} ${response.statusText}`;
      throw new RustFlowError(message, response.status, body);
    }

    return response.json() as Promise<T>;
  }

  // ── Health ──────────────────────────────────────────────────────────────────

  /** `GET /health` — Check server health. */
  async health(): Promise<HealthResponse> {
    return this.request<HealthResponse>("/health");
  }

  // ── Agent CRUD ──────────────────────────────────────────────────────────────

  /** `POST /agents` — Create a new agent from a step list. */
  async createAgent(req: CreateAgentRequest): Promise<CreateAgentResponse> {
    return this.request<CreateAgentResponse>("/agents", {
      method: "POST",
      body: JSON.stringify(req),
    });
  }

  /** `GET /agents` — List all registered agents. */
  async listAgents(): Promise<ListAgentsResponse> {
    return this.request<ListAgentsResponse>("/agents");
  }

  /** `GET /agents/:id` — Get a single agent with its full step list. */
  async getAgent(id: string): Promise<Agent> {
    return this.request<Agent>(`/agents/${encodeURIComponent(id)}`);
  }

  /** `DELETE /agents/:id` — Delete an agent. */
  async deleteAgent(id: string): Promise<DeleteResponse> {
    return this.request<DeleteResponse>(`/agents/${encodeURIComponent(id)}`, {
      method: "DELETE",
    });
  }

  // ── Execution ───────────────────────────────────────────────────────────────

  /**
   * `POST /agents/:id/run` — Execute an agent and wait for the final result.
   *
   * For long-running workflows use `stream()` instead to get per-step events.
   */
  async runAgent(
    id: string,
    vars: Record<string, unknown> = {},
  ): Promise<RunResult> {
    return this.request<RunResult>(`/agents/${encodeURIComponent(id)}/run`, {
      method: "POST",
      body: JSON.stringify({ vars }),
    });
  }

  /**
   * `GET /agents/:id/stream` — Execute an agent and stream execution events
   * via WebSocket.
   *
   * If a run is already active for this agent the client is attached as an
   * observer instead — no duplicate execution is started.
   *
   * Returns an `AsyncGenerator` that yields mid-run `WsEvent`s and returns the
   * terminal `WorkflowCompletedEvent | WorkflowFailedEvent` as its final value.
   *
   * ```ts
   * for await (const event of client.stream("agent-id", { vars: { lang: "en" } })) {
   *   if (event.type === "step_succeeded") console.log(event.output);
   * }
   * ```
   */
  stream(
    id: string,
    options: StreamOptions = {},
  ): AsyncGenerator<WsEvent, WorkflowCompletedEvent | WorkflowFailedEvent> {
    const url = `${this.wsBaseUrl}/agents/${encodeURIComponent(id)}/stream`;
    return this.openWsStream(url, { vars: options.vars ?? {} }, options.onEvent);
  }

  /**
   * `GET /agents/:id/observe` — Attach to an existing run as a read-only
   * observer without starting a new execution.
   *
   * Replays all events emitted so far, then streams live events until the
   * workflow finishes. If no active run exists the generator returns
   * immediately with a `workflow_failed` terminal event.
   *
   * ```ts
   * const terminal = await client.observe("agent-id").next(); // or for-await
   * ```
   */
  observe(
    id: string,
    options: Pick<StreamOptions, "onEvent"> = {},
  ): AsyncGenerator<WsEvent, WorkflowCompletedEvent | WorkflowFailedEvent> {
    const url = `${this.wsBaseUrl}/agents/${encodeURIComponent(id)}/observe`;
    return this.openWsStream(url, {}, options.onEvent);
  }

  // ── Internal WebSocket generator ─────────────────────────────────────────────

  /**
   * Open a WebSocket to `url`, send `startMessage` on connect, then yield
   * all non-terminal `WsEvent`s and return the terminal one.
   */
  private async *openWsStream(
    url: string,
    startMessage: Record<string, unknown>,
    onEvent?: (event: WsEvent) => void,
  ): AsyncGenerator<WsEvent, WorkflowCompletedEvent | WorkflowFailedEvent> {
    const ws = new WebSocket(url);

    // Buffer events that arrive before the consumer calls `next()`.
    const queue: WsEvent[] = [];
    let resolve: ((value: IteratorResult<WsEvent, WorkflowCompletedEvent | WorkflowFailedEvent>) => void) | null = null;
    let terminalEvent: WorkflowCompletedEvent | WorkflowFailedEvent | null = null;
    let errorEvent: Error | null = null;
    let done = false;

    const push = (event: WsEvent) => {
      onEvent?.(event);
      if (resolve) {
        const r = resolve;
        resolve = null;
        r({ value: event, done: false });
      } else {
        queue.push(event);
      }
    };

    const finish = (terminal: WorkflowCompletedEvent | WorkflowFailedEvent) => {
      terminalEvent = terminal;
      done = true;
      if (resolve) {
        const r = resolve;
        resolve = null;
        r({ value: terminal, done: true });
      }
      ws.close();
    };

    const fail = (err: Error) => {
      errorEvent = err;
      done = true;
      if (resolve) {
        const r = resolve;
        resolve = null;
        r({ value: undefined as never, done: true });
      }
    };

    ws.onopen = () => {
      ws.send(JSON.stringify(startMessage));
    };

    ws.onmessage = (ev) => {
      let event: WsEvent;
      try {
        event = JSON.parse(ev.data as string) as WsEvent;
      } catch {
        return;
      }
      if (event.type === "workflow_completed" || event.type === "workflow_failed") {
        finish(event);
      } else {
        push(event);
      }
    };

    ws.onerror = () => {
      fail(new RustFlowError(`WebSocket error connecting to ${url}`));
    };

    ws.onclose = (ev) => {
      if (!done) {
        fail(new RustFlowError(`WebSocket closed unexpectedly (code ${ev.code})`));
      }
    };

    while (true) {
      if (errorEvent) throw errorEvent;
      if (queue.length > 0) {
        yield queue.shift()!;
        continue;
      }
      if (done) {
        return terminalEvent!;
      }
      yield await new Promise<WsEvent>((res, rej) => {
        if (errorEvent) { rej(errorEvent); return; }
        if (queue.length > 0) { res(queue.shift()!); return; }
        if (done) { res(terminalEvent! as unknown as WsEvent); return; }
        resolve = ({ value }) => {
          if (errorEvent) rej(errorEvent);
          else res(value as WsEvent);
        };
      });
    }
  }

  // ── Playground ──────────────────────────────────────────────────────────────

  /**
   * `POST /playground/agents` — Parse a YAML workflow definition and register
   * it as an agent. Returns the new agent's ID.
   */
  async createFromYaml(yaml: string): Promise<CreateAgentResponse> {
    return this.request<CreateAgentResponse>("/playground/agents", {
      method: "POST",
      body: JSON.stringify({ yaml }),
    });
  }
}

// ── Step builder helpers ──────────────────────────────────────────────────────

/**
 * Build a tool step definition.
 *
 * ```ts
 * const step = toolStep("fetch", "Fetch Data", "http", {
 *   url: "https://api.example.com/data",
 *   method: "GET",
 * });
 * ```
 */
export function toolStep(
  id: string,
  name: string,
  tool: string,
  input: Record<string, unknown>,
  options: StepBuilderOptions = {},
): Step {
  return {
    id,
    name,
    kind: { tool: { tool, input } satisfies ToolConfig } satisfies StepKind,
    depends_on: options.depends_on ?? [],
    retry_policy: options.retry_policy ?? { kind: "none" },
    timeout_ms: options.timeout_ms,
  };
}

/**
 * Build an LLM step definition.
 *
 * ```ts
 * const step = llmStep("summarise", "Summarise", {
 *   provider: "ollama",
 *   model: "llama3",
 *   prompt: "Summarise: {{steps.fetch.output}}",
 * }, { depends_on: ["fetch"] });
 * ```
 */
export function llmStep(
  id: string,
  name: string,
  config: LlmConfig,
  options: StepBuilderOptions = {},
): Step {
  return {
    id,
    name,
    kind: { llm: config } satisfies StepKind,
    depends_on: options.depends_on ?? [],
    retry_policy: options.retry_policy ?? { kind: "none" },
    timeout_ms: options.timeout_ms,
  };
}

export type { AgentSummary, Agent, Step, WsEvent };
