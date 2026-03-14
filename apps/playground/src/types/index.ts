// ── WebSocket event types ─────────────────────────────────────────────────────

export type WsEventType =
  | 'step_started'
  | 'step_succeeded'
  | 'step_failed'
  | 'step_retrying'
  | 'circuit_breaker_opened'
  | 'circuit_breaker_closed'
  | 'workflow_completed'
  | 'workflow_failed';

export interface WsEventBase {
  type: WsEventType;
}

export interface WsStepStarted extends WsEventBase {
  type: 'step_started';
  step_id: string;
  step_name: string;
}

export interface WsStepSucceeded extends WsEventBase {
  type: 'step_succeeded';
  step_id: string;
  step_name: string;
  elapsed_ms: number;
  output: unknown;
}

export interface WsStepFailed extends WsEventBase {
  type: 'step_failed';
  step_id: string;
  step_name: string;
  error: string;
  will_retry: boolean;
  attempt: number;
  elapsed_ms: number;
}

export interface WsStepRetrying extends WsEventBase {
  type: 'step_retrying';
  step_id: string;
  step_name: string;
  attempt: number;
}

export interface WsCircuitBreakerOpened extends WsEventBase {
  type: 'circuit_breaker_opened';
  resource: string;
}

export interface WsCircuitBreakerClosed extends WsEventBase {
  type: 'circuit_breaker_closed';
  resource: string;
}

export interface WsWorkflowCompleted extends WsEventBase {
  type: 'workflow_completed';
  outputs: Record<string, unknown>;
}

export interface WsWorkflowFailed extends WsEventBase {
  type: 'workflow_failed';
  error: string;
}

export type WsEvent =
  | WsStepStarted
  | WsStepSucceeded
  | WsStepFailed
  | WsStepRetrying
  | WsCircuitBreakerOpened
  | WsCircuitBreakerClosed
  | WsWorkflowCompleted
  | WsWorkflowFailed;

// ── Agent types ───────────────────────────────────────────────────────────────

export interface AgentSummary {
  id: string;
  name: string;
  description?: string;
  step_count: number;
  created_at: string;
}

// ── Step execution state ──────────────────────────────────────────────────────

export type StepStatus = 'pending' | 'running' | 'retrying' | 'success' | 'failed';

export interface StepState {
  id: string;
  name: string;
  status: StepStatus;
  elapsed_ms?: number;
  output?: unknown;
  error?: string;
  attempt?: number;
  startedAt?: number;
}

// ── Run status ────────────────────────────────────────────────────────────────

export type RunStatus = 'idle' | 'running' | 'completed' | 'failed';

// ── System messages ───────────────────────────────────────────────────────────

export interface SystemMessage {
  id: string;
  text: string;
  type: 'info' | 'warning';
}
