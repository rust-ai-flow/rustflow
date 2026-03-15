import React, { useCallback, useEffect, useState } from 'react';
import jsyaml from 'js-yaml';
import { RustFlowError } from 'rustflow';
import { Sidebar } from './components/Sidebar';
import { WorkflowEditor, SAMPLES } from './components/WorkflowEditor';
import { ExecutionPanel } from './components/ExecutionPanel';
import { useAgents } from './hooks/useAgents';
import { useRunManager } from './hooks/useRunManager';
import { client } from './lib/client';

// ── Toast ─────────────────────────────────────────────────────────────────────

interface Toast {
  id: string;
  message: string;
  type: 'info' | 'success' | 'error';
}

function ToastContainer({ toasts, onRemove }: { toasts: Toast[]; onRemove: (id: string) => void }) {
  return (
    <div className="fixed top-16 left-1/2 -translate-x-1/2 z-50 flex flex-col gap-2">
      {toasts.map(toast => (
        <div
          key={toast.id}
          className="flex items-center gap-2 px-4 py-3 rounded-lg shadow-lg text-sm max-w-md cursor-pointer"
          style={{
            background: '#1E293B',
            color: '#F8FAFC',
            borderLeft: `3px solid ${toast.type === 'error' ? '#EF4444' : toast.type === 'success' ? '#22C55E' : '#F97316'}`,
          }}
          onClick={() => onRemove(toast.id)}
        >
          {toast.message}
        </div>
      ))}
    </div>
  );
}

// ── App ───────────────────────────────────────────────────────────────────────

export default function App() {
  const [yaml, setYaml] = useState(SAMPLES.hello);
  const [vars, setVars] = useState('{}');
  const [workflowName, setWorkflowName] = useState('hello-playground');
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);

  const { agents, refresh: refreshAgents } = useAgents(5000);

  const showToast = useCallback((message: string, type: Toast['type'] = 'info') => {
    const id = Math.random().toString(36).slice(2);
    setToasts(prev => [...prev, { id, message, type }]);
    setTimeout(() => setToasts(prev => prev.filter(t => t.id !== id)), 3000);
  }, []);

  const removeToast = useCallback((id: string) => {
    setToasts(prev => prev.filter(t => t.id !== id));
  }, []);

  const runManagerOptions = React.useMemo(() => ({
    onCompleted: (_agentId: string, _outputs: Record<string, unknown>) => {
      showToast('Workflow completed', 'success');
      void refreshAgents();
    },
    onFailed: (_agentId: string, error: string) => {
      showToast(`Workflow failed: ${error}`, 'error');
    },
  }), [showToast, refreshAgents]);

  const { getSnapshot, activeRuns, startRun } = useRunManager(runManagerOptions);

  // Update breadcrumb when YAML changes.
  useEffect(() => {
    try {
      const doc = jsyaml.load(yaml) as { name?: string } | null;
      if (doc?.name) setWorkflowName(doc.name);
    } catch { /* ignore */ }
  }, [yaml]);

  const runWorkflow = async () => {
    try { jsyaml.load(yaml); }
    catch (e: unknown) {
      showToast('YAML parse error: ' + (e instanceof Error ? e.message : String(e)), 'error');
      return;
    }

    let parsedVars: Record<string, unknown> = {};
    try {
      const raw = vars.trim();
      if (raw) parsedVars = JSON.parse(raw) as Record<string, unknown>;
    } catch {
      showToast('Invalid vars JSON', 'error');
      return;
    }

    try {
      const { id } = await client.createFromYaml(yaml);
      setSelectedAgentId(id);
      startRun(id, parsedVars);
    } catch (e: unknown) {
      showToast(e instanceof RustFlowError ? e.message : String(e), 'error');
    }
  };

  const handleNew = () => {
    setYaml(SAMPLES.hello);
    setSelectedAgentId(null);
  };

  const handleAgentSelect = async (agentId: string) => {
    try {
      const agent = await client.getAgent(agentId);
      const doc = {
        name: agent.name,
        ...(agent.description ? { description: agent.description } : {}),
        steps: agent.steps.map(step => {
          const s: Record<string, unknown> = { id: step.id, name: step.name };
          if ('llm' in step.kind) {
            s.llm = step.kind.llm;
          } else {
            s.tool = { name: step.kind.tool.tool, input: step.kind.tool.input };
          }
          if (step.depends_on.length > 0) s.depends_on = step.depends_on;
          if (step.retry_policy.kind !== 'none') s.retry = step.retry_policy;
          if (step.timeout_ms != null) s.timeout_ms = step.timeout_ms;
          return s;
        }),
      };
      setYaml(jsyaml.dump(doc, { indent: 2, lineWidth: 120 }));
      setSelectedAgentId(agentId);
    } catch (e: unknown) {
      showToast(e instanceof RustFlowError ? e.message : String(e), 'error');
    }
  };

  const handleVarsKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') void runWorkflow();
  };

  const displayState = getSnapshot(selectedAgentId);
  const isCurrentRunning = selectedAgentId !== null && activeRuns.has(selectedAgentId);

  return (
    <div className="h-screen flex flex-col overflow-hidden" style={{ fontFamily: "'Inter', sans-serif" }}>
      {/* Top header */}
      <header className="flex items-center px-4 bg-white border-b border-slate-200 shrink-0" style={{ height: 44 }}>
        <div className="flex items-center gap-2 flex-1 min-w-0">
          <span className="text-slate-400 text-sm">Playground</span>
          <span className="text-slate-300">/</span>
          <span className="text-sm font-medium text-slate-700 truncate">{workflowName}</span>
        </div>

        <div className="flex items-center gap-2 mx-4">
          <label className="text-xs text-slate-500 font-medium">vars</label>
          <input
            type="text"
            value={vars}
            onChange={e => setVars(e.target.value)}
            onKeyDown={handleVarsKeyDown}
            placeholder='{"lang":"en"}'
            className="font-mono text-xs bg-slate-50 border border-slate-200 rounded px-2 py-1 text-slate-700 w-36 focus:outline-none focus:border-orange-400 focus:ring-1 focus:ring-orange-400"
          />
        </div>

        <button
          onClick={() => void runWorkflow()}
          disabled={isCurrentRunning}
          className="flex items-center gap-2 px-4 py-1.5 text-white text-sm font-medium rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          style={{ background: '#F97316' }}
        >
          {isCurrentRunning ? (
            <svg className="animate-spin" width="14" height="14" viewBox="0 0 14 14" fill="none">
              <circle cx="7" cy="7" r="6" stroke="rgba(255,255,255,0.3)" strokeWidth="2" />
              <path d="M7 1 A6 6 0 0 1 13 7" stroke="white" strokeWidth="2" strokeLinecap="round" />
            </svg>
          ) : (
            <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
              <polygon points="3,1 13,7 3,13" />
            </svg>
          )}
          {isCurrentRunning ? 'Running...' : 'Run'}
        </button>
      </header>

      {/* 3-column layout */}
      <div className="flex flex-1 min-h-0">
        <Sidebar
          agents={agents}
          selectedAgentId={selectedAgentId}
          activeRuns={activeRuns}
          onNew={handleNew}
          onAgentSelect={handleAgentSelect}
        />
        <WorkflowEditor value={yaml} onChange={setYaml} onToast={showToast} />
        <ExecutionPanel
          runStatus={displayState.runStatus}
          steps={displayState.steps}
          systemMessages={displayState.systemMessages}
          outputs={displayState.outputs}
        />
      </div>

      <ToastContainer toasts={toasts} onRemove={removeToast} />
    </div>
  );
}
