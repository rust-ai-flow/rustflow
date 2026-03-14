import { useCallback, useRef, useState } from 'react';
import type { RunStatus, StepState, SystemMessage, WsEvent } from '../types';

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
  const wsRef = useRef<WebSocket | null>(null);
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
      setSteps(prev =>
        prev.map(s => s.id === stepId ? { ...s, elapsed_ms: elapsed } : s)
      );
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
              s.id === event.step_id
                ? { ...s, status: 'running', startedAt: Date.now() }
                : s
            );
          }
          return [...prev, {
            id: event.step_id,
            name: event.step_name,
            status: 'running',
            startedAt: Date.now(),
          }];
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
              s.id === event.step_id
                ? { ...s, status: 'retrying', attempt: event.attempt }
                : s
            )
          );
        }
        break;

      case 'step_retrying':
        setSteps(prev =>
          prev.map(s =>
            s.id === event.step_id
              ? { ...s, status: 'retrying', attempt: event.attempt }
              : s
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

      case 'workflow_completed':
        setRunStatus('completed');
        setOutputs(event.outputs);
        options.onCompleted?.(event.outputs);
        break;

      case 'workflow_failed':
        setRunStatus('failed');
        options.onFailed?.(event.error);
        break;
    }
  }, [startTimer, stopTimer, options]);

  const connect = useCallback((agentId: string, vars: Record<string, unknown>) => {
    disconnect();

    const wsProtocol = 'ws:';
    const wsUrl = `${wsProtocol}//localhost:8080/agents/${agentId}/stream`;
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => {
      ws.send(JSON.stringify({ vars }));
    };

    ws.onmessage = (evt) => {
      try {
        const event: WsEvent = JSON.parse(evt.data as string);
        handleEvent(event);
      } catch (e) {
        console.warn('Failed to parse WS event', e);
      }
    };

    ws.onerror = () => {
      setRunStatus('failed');
      options.onFailed?.('WebSocket connection error');
    };

    ws.onclose = () => {
      wsRef.current = null;
    };
  }, [handleEvent, options]);

  const disconnect = useCallback(() => {
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    // Stop all timers
    Object.keys(timerRefs.current).forEach(stopTimer);
  }, [stopTimer]);

  const reset = useCallback(() => {
    disconnect();
    setRunStatus('idle');
    setSteps([]);
    setSystemMessages([]);
    setOutputs(null);
  }, [disconnect]);

  return { runStatus, steps, systemMessages, outputs, connect, disconnect, reset };
}
