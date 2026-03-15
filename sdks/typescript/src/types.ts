// ── Core domain types ─────────────────────────────────────────────────────────

export interface HealthResponse {
  status: string;
  version: string;
}

export interface AgentSummary {
  id: string;
  name: string;
  description?: string;
  step_count: number;
  created_at: string;
}

// ── Retry policy ─────────────────────────────────────────────────────────────

export type RetryPolicy =
  | { kind: "none" }
  | { kind: "fixed"; max_retries: number; interval_ms: number }
  | {
      kind: "exponential";
      max_retries: number;
      initial_interval_ms: number;
      multiplier: number;
      max_interval_ms: number;
    };

// ── Step types ────────────────────────────────────────────────────────────────

export interface LlmConfig {
  provider: string;
  model: string;
  prompt: string;
  temperature?: number;
  max_tokens?: number;
}

export interface ToolConfig {
  tool: string;
  input: Record<string, unknown>;
}

/** Externally-tagged: `{ "llm": {...} }` or `{ "tool": {...} }` */
export type StepKind = { llm: LlmConfig } | { tool: ToolConfig };

export type StepState = "pending" | "running" | "success" | "failed" | "retrying";

export interface Step {
  id: string;
  name: string;
  kind: StepKind;
  depends_on: string[];
  retry_policy: RetryPolicy;
  timeout_ms?: number;
}

export interface Agent extends AgentSummary {
  steps: Step[];
}

// ── Request / response shapes ─────────────────────────────────────────────────

export interface CreateAgentRequest {
  name: string;
  description?: string;
  steps: Step[];
}

export interface CreateAgentResponse {
  id: string;
  message: string;
}

export interface ListAgentsResponse {
  agents: AgentSummary[];
  count: number;
}

export interface RunResult {
  agent_id: string;
  status: string;
  outputs: Record<string, unknown>;
}

export interface DeleteResponse {
  message: string;
}

// ── WebSocket event types ─────────────────────────────────────────────────────

export interface StepStartedEvent {
  type: "step_started";
  step_id: string;
  step_name: string;
}

export interface StepSucceededEvent {
  type: "step_succeeded";
  step_id: string;
  step_name: string;
  elapsed_ms: number;
  output: unknown;
}

export interface StepFailedEvent {
  type: "step_failed";
  step_id: string;
  step_name: string;
  error: string;
  will_retry: boolean;
  attempt: number;
  elapsed_ms: number;
}

export interface StepRetryingEvent {
  type: "step_retrying";
  step_id: string;
  step_name: string;
  attempt: number;
}

export interface CircuitBreakerOpenedEvent {
  type: "circuit_breaker_opened";
  resource: string;
}

export interface CircuitBreakerClosedEvent {
  type: "circuit_breaker_closed";
  resource: string;
}

export interface WorkflowCompletedEvent {
  type: "workflow_completed";
  outputs: Record<string, unknown>;
}

export interface WorkflowFailedEvent {
  type: "workflow_failed";
  error: string;
}

export type WsEvent =
  | StepStartedEvent
  | StepSucceededEvent
  | StepFailedEvent
  | StepRetryingEvent
  | CircuitBreakerOpenedEvent
  | CircuitBreakerClosedEvent
  | WorkflowCompletedEvent
  | WorkflowFailedEvent;

// ── Builder helpers ───────────────────────────────────────────────────────────

export interface StepBuilderOptions {
  depends_on?: string[];
  retry_policy?: RetryPolicy;
  timeout_ms?: number;
}
