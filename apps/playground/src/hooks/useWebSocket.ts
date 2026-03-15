import { useCallback, useRef, useState } from 'react';
import type { WsEvent } from 'rustflow';
import { client } from '../lib/client';
import type { RunStatus, StepState, SystemMessage } from '../types';

interface UseWebSocketOptions {
  onCompleted?: (outputs: Record<string, unknown>) => void;
  onFailed?: (error: string) => void;
}

interface UseWebSocketReturn {
  runStatus: RunStatus;
  steps: StepState[];
  systemMessages: SystemMessage[];
  outputs: Record<string, unknown> | null;
  connect: (agentId: string, vars: Record<string, unknown>) => void;
  disconnect: () => void;
  reset: () => void;
}

export function useWebSocket(options: UseWebSocketOptions = {}): UseWebSocketReturn {
  // Ref to the current stream generator so we can cancel it on disconnect.
  const generatorRef = useRef<AsyncGenerator<WsEvent> | null>(null);
  const timerRefs = useRef<Record<string, number>>({});
  const startTimesRef = useRef<Record<string, number>>({});

  const [runStatus, setRunStatus] = useState<RunStatus>('idle');
  const [steps, setSteps] = useState<StepState[]>([]);
  const [systemMessages, setSystemMessages] = useState<SystemMessage[]>([]);
  const [outputs, setOutputs] = useState<Record<string, unknown> | null>(null);

  const stopTimer = useCallback((stepId: string) => {
    const tid = timerRefs.current[stepId];
    if (tid != null) {
      clearInterval(tid);
      delete timerRefs.current[stepId];
    }
  }, []);

  const startTimer = useCallback((stepId: string) => {
    stopTimer(stepId);
    startTimesRef.current[stepId] = Date.now();
    const tid = window.setInterval(() => {
      const elapsed = Date.now() - (startTimesRef.current[stepId] ?? Date.now());
      setSteps(prev => prev.map(s => s.id === stepId ? { ...s, elapsed_ms: elapsed } : s));
    }, 100);
    timerRefs.current[stepId] = tid;
  }, [stopTimer]);

  const handleEvent = useCallback((event: WsEvent) => {
    switch (event.type) {
      case 'step_started':
        setSteps(prev => {
          const existing = prev.find(s => s.id === event.step_id);
          if (existing) {
            return prev.map(s =>
              s.id === event.step_id ? { ...s, status: 'running', startedAt: Date.now() } : s
            );
          }
          return [...prev, { id: event.step_id, name: event.step_name, status: 'running', startedAt: Date.now() }];
        });
        startTimer(event.step_id);
        break;

      case 'step_succeeded':
        stopTimer(event.step_id);
        setSteps(prev =>
          prev.map(s =>
            s.id === event.step_id
              ? { ...s, status: 'success', elapsed_ms: event.elapsed_ms, output: event.output }
              : s
          )
        );
        break;

      case 'step_failed':
        if (!event.will_retry) {
          stopTimer(event.step_id);
          setSteps(prev =>
            prev.map(s =>
              s.id === event.step_id
                ? { ...s, status: 'failed', elapsed_ms: event.elapsed_ms, error: event.error }
                : s
            )
          );
        } else {
          setSteps(prev =>
            prev.map(s =>
              s.id === event.step_id ? { ...s, status: 'retrying', attempt: event.attempt } : s
            )
          );
        }
        break;

      case 'step_retrying':
        setSteps(prev =>
          prev.map(s =>
            s.id === event.step_id ? { ...s, status: 'retrying', attempt: event.attempt } : s
          )
        );
        break;

      case 'circuit_breaker_opened':
        setSystemMessages(prev => [...prev, {
          id: `cb-open-${event.resource}-${Date.now()}`,
          text: `Circuit breaker opened for: ${event.resource}`,
          type: 'warning',
        }]);
        break;

      case 'circuit_breaker_closed':
        setSystemMessages(prev => [...prev, {
          id: `cb-close-${event.resource}-${Date.now()}`,
          text: `Circuit breaker closed for: ${event.resource}`,
          type: 'info',
        }]);
        break;

      // Terminal events (workflow_completed / workflow_failed) are only handled as generator return values
      // They should not be yielded as regular events
    }
  }, [startTimer, stopTimer]);

  const disconnect = useCallback(() => {
    if (generatorRef.current) {
      void generatorRef.current.return(undefined);
      generatorRef.current = null;
    }
    Object.keys(timerRefs.current).forEach(stopTimer);
  }, [stopTimer]);

  const connect = useCallback((agentId: string, vars: Record<string, unknown>) => {
    disconnect();
    setRunStatus('running');

    // client.stream() returns an AsyncGenerator<WsEvent, WorkflowCompletedEvent | WorkflowFailedEvent>.
    // Terminal events (workflow_completed / workflow_failed) are the generator's return value,
    // not yielded iterations, so we use the manual .next() loop to handle both.
    const gen = client.stream(agentId, { vars });
    generatorRef.current = gen;

    const run = async () => {
      try {
        let result = await gen.next();
        while (!result.done) {
          handleEvent(result.value);
          result = await gen.next();
        }
        // Terminal event (workflow_completed or workflow_failed)
        if (result.value) {
          if (result.value.type === 'workflow_completed') {
            setRunStatus('completed');
            setOutputs(result.value.outputs);
            options.onCompleted?.(result.value.outputs);
          } else if (result.value.type === 'workflow_failed') {
            setRunStatus('failed');
            options.onFailed?.(result.value.error);
          }
        }
      } catch (e: unknown) {
        // Generator was cancelled via .return() — ignore.
        if (generatorRef.current === null) return;
        setRunStatus('failed');
        options.onFailed?.(e instanceof Error ? e.message : String(e));
      }
    };

    void run();
  }, [disconnect, handleEvent, options]);

  const reset = useCallback(() => {
    disconnect();
    setRunStatus('idle');
    setSteps([]);
    setSystemMessages([]);
    setOutputs(null);
  }, [disconnect]);

  return { runStatus, steps, systemMessages, outputs, connect, disconnect, reset };
}
