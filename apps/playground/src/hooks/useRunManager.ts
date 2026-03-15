import { useCallback, useEffect, useRef, useState } from 'react';
import type { WsEvent } from 'rustflow';
import { client } from '../lib/client';
import type { ExecutionSnapshot, RunStatus, StepState, SystemMessage } from '../types';

const STORAGE_KEY = 'rustflow:snapshots';
const MAX_SNAPSHOTS = 50; // keep the most recent N agents to avoid unbounded growth

const EMPTY_SNAPSHOT: ExecutionSnapshot = {
  runStatus: 'idle',
  steps: [],
  systemMessages: [],
  outputs: null,
};

function loadSnapshots(): Record<string, ExecutionSnapshot> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const data = JSON.parse(raw) as Record<string, ExecutionSnapshot>;
    // Any run that was "running" when the page closed is now interrupted.
    for (const id of Object.keys(data)) {
      if (data[id].runStatus === 'running') {
        data[id] = { ...data[id], runStatus: 'interrupted' };
      }
    }
    return data;
  } catch {
    return {};
  }
}

function saveSnapshots(snapshots: Record<string, ExecutionSnapshot>) {
  try {
    // Trim to the most recent MAX_SNAPSHOTS entries (by key insertion order).
    const keys = Object.keys(snapshots);
    const trimmed = keys.length > MAX_SNAPSHOTS
      ? Object.fromEntries(keys.slice(-MAX_SNAPSHOTS).map(k => [k, snapshots[k]]))
      : snapshots;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(trimmed));
  } catch {
    // Storage quota exceeded — ignore.
  }
}

interface RunState {
  gen: AsyncGenerator<WsEvent>;
  timers: Record<string, number>;
  startTimes: Record<string, number>;
}

interface UseRunManagerReturn {
  snapshots: Record<string, ExecutionSnapshot>;
  activeRuns: Set<string>;
  getSnapshot: (agentId: string | null) => ExecutionSnapshot;
  startRun: (agentId: string, vars: Record<string, unknown>) => void;
  cancelRun: (agentId: string) => void;
  onCompleted?: (agentId: string, outputs: Record<string, unknown>) => void;
  onFailed?: (agentId: string, error: string) => void;
}

interface UseRunManagerOptions {
  onCompleted?: (agentId: string, outputs: Record<string, unknown>) => void;
  onFailed?: (agentId: string, error: string) => void;
}

export function useRunManager(options: UseRunManagerOptions = {}): UseRunManagerReturn {
  const [snapshots, setSnapshots] = useState<Record<string, ExecutionSnapshot>>(loadSnapshots);
  const [activeRuns, setActiveRuns] = useState<Set<string>>(new Set());

  // Keyed by agentId — holds the generator and per-run timers.
  const runsRef = useRef<Map<string, RunState>>(new Map());

  // Persist to localStorage whenever snapshots change.
  useEffect(() => {
    saveSnapshots(snapshots);
  }, [snapshots]);

  const updateSnapshot = useCallback((agentId: string, updater: (prev: ExecutionSnapshot) => ExecutionSnapshot) => {
    setSnapshots(prev => ({
      ...prev,
      [agentId]: updater(prev[agentId] ?? EMPTY_SNAPSHOT),
    }));
  }, []);

  const stopTimers = useCallback((agentId: string) => {
    const run = runsRef.current.get(agentId);
    if (!run) return;
    Object.values(run.timers).forEach(tid => clearInterval(tid));
    run.timers = {};
  }, []);

  const stopStepTimer = useCallback((agentId: string, stepId: string) => {
    const run = runsRef.current.get(agentId);
    if (!run) return;
    const tid = run.timers[stepId];
    if (tid != null) {
      clearInterval(tid);
      delete run.timers[stepId];
    }
  }, []);

  const startStepTimer = useCallback((agentId: string, stepId: string) => {
    stopStepTimer(agentId, stepId);
    const run = runsRef.current.get(agentId);
    if (!run) return;
    run.startTimes[stepId] = Date.now();
    const tid = window.setInterval(() => {
      const elapsed = Date.now() - (run.startTimes[stepId] ?? Date.now());
      setSnapshots(prev => {
        const snap = prev[agentId];
        if (!snap) return prev;
        return {
          ...prev,
          [agentId]: {
            ...snap,
            steps: snap.steps.map(s => s.id === stepId ? { ...s, elapsed_ms: elapsed } : s),
          },
        };
      });
    }, 100);
    run.timers[stepId] = tid;
  }, [stopStepTimer]);

  const handleEvent = useCallback((agentId: string, event: WsEvent) => {
    switch (event.type) {
      case 'step_started':
        updateSnapshot(agentId, snap => {
          const existing = snap.steps.find((s: StepState) => s.id === event.step_id);
          const steps = existing
            ? snap.steps.map((s: StepState) =>
                s.id === event.step_id ? { ...s, status: 'running' as const, startedAt: Date.now() } : s
              )
            : [...snap.steps, { id: event.step_id, name: event.step_name, status: 'running' as const, startedAt: Date.now() }];
          return { ...snap, steps };
        });
        startStepTimer(agentId, event.step_id);
        break;

      case 'step_succeeded':
        stopStepTimer(agentId, event.step_id);
        updateSnapshot(agentId, snap => ({
          ...snap,
          steps: snap.steps.map((s: StepState) =>
            s.id === event.step_id
              ? { ...s, status: 'success' as const, elapsed_ms: event.elapsed_ms, output: event.output }
              : s
          ),
        }));
        break;

      case 'step_failed':
        if (!event.will_retry) {
          stopStepTimer(agentId, event.step_id);
          updateSnapshot(agentId, snap => ({
            ...snap,
            steps: snap.steps.map((s: StepState) =>
              s.id === event.step_id
                ? { ...s, status: 'failed' as const, elapsed_ms: event.elapsed_ms, error: event.error }
                : s
            ),
          }));
        } else {
          updateSnapshot(agentId, snap => ({
            ...snap,
            steps: snap.steps.map((s: StepState) =>
              s.id === event.step_id ? { ...s, status: 'retrying' as const, attempt: event.attempt } : s
            ),
          }));
        }
        break;

      case 'step_retrying':
        updateSnapshot(agentId, snap => ({
          ...snap,
          steps: snap.steps.map((s: StepState) =>
            s.id === event.step_id ? { ...s, status: 'retrying' as const, attempt: event.attempt } : s
          ),
        }));
        break;

      case 'circuit_breaker_opened': {
        const msg: SystemMessage = {
          id: `cb-open-${event.resource}-${Date.now()}`,
          text: `Circuit breaker opened for: ${event.resource}`,
          type: 'warning',
        };
        updateSnapshot(agentId, snap => ({ ...snap, systemMessages: [...snap.systemMessages, msg] }));
        break;
      }

      case 'circuit_breaker_closed': {
        const msg: SystemMessage = {
          id: `cb-close-${event.resource}-${Date.now()}`,
          text: `Circuit breaker closed for: ${event.resource}`,
          type: 'info',
        };
        updateSnapshot(agentId, snap => ({ ...snap, systemMessages: [...snap.systemMessages, msg] }));
        break;
      }
    }
  }, [updateSnapshot, startStepTimer, stopStepTimer]);

  const cancelRun = useCallback((agentId: string) => {
    const run = runsRef.current.get(agentId);
    if (run) {
      stopTimers(agentId);
      void run.gen.return(undefined);
      runsRef.current.delete(agentId);
    }
    setActiveRuns(prev => {
      const next = new Set(prev);
      next.delete(agentId);
      return next;
    });
  }, [stopTimers]);

  const startRun = useCallback((agentId: string, vars: Record<string, unknown>) => {
    // Cancel any existing run for this agent.
    cancelRun(agentId);

    // Reset snapshot to running state.
    setSnapshots(prev => ({
      ...prev,
      [agentId]: { runStatus: 'running', steps: [], systemMessages: [], outputs: null },
    }));
    setActiveRuns(prev => new Set([...prev, agentId]));

    const gen = client.stream(agentId, { vars });
    const runState: RunState = { gen, timers: {}, startTimes: {} };
    runsRef.current.set(agentId, runState);

    const run = async () => {
      try {
        let result = await gen.next();
        while (!result.done) {
          handleEvent(agentId, result.value);
          result = await gen.next();
        }
        // Terminal event
        if (result.value) {
          if (result.value.type === 'workflow_completed') {
            const outputs = result.value.outputs;
            updateSnapshot(agentId, snap => ({ ...snap, runStatus: 'completed' as RunStatus, outputs }));
            options.onCompleted?.(agentId, outputs);
          } else if (result.value.type === 'workflow_failed') {
            const error = result.value.error;
            updateSnapshot(agentId, snap => ({ ...snap, runStatus: 'failed' as RunStatus }));
            options.onFailed?.(agentId, error);
          }
        }
      } catch (e: unknown) {
        // Cancelled via .return() — skip.
        if (!runsRef.current.has(agentId)) return;
        updateSnapshot(agentId, snap => ({ ...snap, runStatus: 'failed' as RunStatus }));
        options.onFailed?.(agentId, e instanceof Error ? e.message : String(e));
      } finally {
        stopTimers(agentId);
        runsRef.current.delete(agentId);
        setActiveRuns(prev => {
          const next = new Set(prev);
          next.delete(agentId);
          return next;
        });
      }
    };

    void run();
  }, [cancelRun, handleEvent, updateSnapshot, stopTimers, options]);

  // Reconnect to an existing run on the server without resetting the snapshot.
  // Replays all past events then continues live — if the server has no active
  // run the generator returns immediately with a workflow_failed terminal event.
  const reconnectRun = useCallback((agentId: string) => {
    cancelRun(agentId);

    // Switch to running state; replay will rebuild steps from scratch.
    setSnapshots(prev => ({
      ...prev,
      [agentId]: { runStatus: 'running', steps: [], systemMessages: [], outputs: null },
    }));
    setActiveRuns(prev => new Set([...prev, agentId]));

    const gen = client.observe(agentId);
    const runState: RunState = { gen: gen as AsyncGenerator<WsEvent>, timers: {}, startTimes: {} };
    runsRef.current.set(agentId, runState);

    const run = async () => {
      try {
        let result = await gen.next();
        while (!result.done) {
          handleEvent(agentId, result.value);
          result = await gen.next();
        }
        if (result.value) {
          if (result.value.type === 'workflow_completed') {
            const outputs = result.value.outputs;
            updateSnapshot(agentId, snap => ({ ...snap, runStatus: 'completed' as RunStatus, outputs }));
            options.onCompleted?.(agentId, outputs);
          } else {
            const error = result.value.error;
            // "no active run" means the server has no record — restore to interrupted.
            const noRun = error.startsWith('no active run');
            updateSnapshot(agentId, snap => ({
              ...snap,
              runStatus: noRun ? ('interrupted' as RunStatus) : ('failed' as RunStatus),
            }));
            if (!noRun) options.onFailed?.(agentId, error);
          }
        }
      } catch (e: unknown) {
        if (!runsRef.current.has(agentId)) return;
        updateSnapshot(agentId, snap => ({ ...snap, runStatus: 'interrupted' as RunStatus }));
      } finally {
        stopTimers(agentId);
        runsRef.current.delete(agentId);
        setActiveRuns(prev => {
          const next = new Set(prev);
          next.delete(agentId);
          return next;
        });
      }
    };

    void run();
  }, [cancelRun, handleEvent, updateSnapshot, stopTimers, options]);

  // On mount, reconnect any snapshots that were interrupted by a page refresh.
  useEffect(() => {
    const interrupted = Object.entries(loadSnapshots())
      .filter(([, snap]) => snap.runStatus === 'interrupted')
      .map(([id]) => id);
    for (const agentId of interrupted) {
      reconnectRun(agentId);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []); // intentionally run once on mount only

  const getSnapshot = useCallback((agentId: string | null): ExecutionSnapshot => {
    if (agentId === null) return EMPTY_SNAPSHOT;
    return snapshots[agentId] ?? EMPTY_SNAPSHOT;
  }, [snapshots]);

  return { snapshots, activeRuns, getSnapshot, startRun, cancelRun };
}
