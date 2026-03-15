// Re-export shared types from the SDK — no need to duplicate them here.
export type {
  WsEvent,
  AgentSummary,
  StepStartedEvent,
  StepSucceededEvent,
  StepFailedEvent,
  StepRetryingEvent,
  CircuitBreakerOpenedEvent,
  CircuitBreakerClosedEvent,
  WorkflowCompletedEvent,
  WorkflowFailedEvent,
} from 'rustflow';

// ── UI-only types ─────────────────────────────────────────────────────────────

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

export type RunStatus = 'idle' | 'running' | 'completed' | 'failed' | 'interrupted';

export interface SystemMessage {
  id: string;
  text: string;
  type: 'info' | 'warning';
}

export interface ExecutionSnapshot {
  runStatus: RunStatus;
  steps: StepState[];
  systemMessages: SystemMessage[];
  outputs: Record<string, unknown> | null;
}
