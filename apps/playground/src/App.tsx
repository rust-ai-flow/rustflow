import React, { useCallback, useEffect, useState } from 'react';
import jsyaml from 'js-yaml';
import { Sidebar } from './components/Sidebar';
import { WorkflowEditor, SAMPLES } from './components/WorkflowEditor';
import { ExecutionPanel } from './components/ExecutionPanel';
import { useAgents } from './hooks/useAgents';
import { useWebSocket } from './hooks/useWebSocket';

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
          className={`flex items-center gap-2 px-4 py-3 rounded-lg shadow-lg text-sm max-w-md cursor-pointer`}
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

  const { agents, refresh: refreshAgents } = useAgents(5000);

  const showToast = useCallback((message: string, type: Toast['type'] = 'info') => {
    const id = Math.random().toString(36).slice(2);
    setToasts(prev => [...prev, { id, message, type }]);
    setTimeout(() => setToasts(prev => prev.filter(t => t.id !== id)), 3000);
  }, []);

  const removeToast = useCallback((id: string) => {
    setToasts(prev => prev.filter(t => t.id !== id));
  }, []);

  // Update breadcrumb name when YAML changes
  useEffect(() => {
    try {
      const doc = jsyaml.load(yaml) as { name?: string } | null;
      if (doc?.name) setWorkflowName(doc.name);
    } catch { /* ignore */ }
  }, [yaml]);

  const ws = useWebSocket({
    onCompleted: () => {
      showToast('Workflow completed', 'success');
      void refreshAgents();
    },
    onFailed: (error) => {
      showToast(`Workflow failed: ${error}`, 'error');
    },
  });

  const [isRunning, setIsRunning] = useState(false);

  const runWorkflow = async () => {
    if (isRunning) return;

    // Validate YAML
    try {
      jsyaml.load(yaml);
    } catch (e: unknown) {
      showToast('YAML parse error: ' + (e instanceof Error ? e.message : String(e)), 'error');
      return;
    }

    // Parse vars
    let parsedVars: Record<string, unknown> = {};
    try {
      const raw = vars.trim();
      if (raw) parsedVars = JSON.parse(raw) as Record<string, unknown>;
    } catch {
      showToast('Invalid vars JSON', 'error');
      return;
    }

    setIsRunning(true);
    ws.reset();

    try {
      const createRes = await fetch('/playground/agents', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ yaml }),
      });

      if (!createRes.ok) {
        const errData = await createRes.json().catch(() => ({ error: 'Unknown error' })) as { error?: string };
        throw new Error(errData.error ?? `Server error: ${createRes.status}`);
      }

      const { id } = await createRes.json() as { id: string };
      ws.connect(id, parsedVars);
    } catch (e: unknown) {
      showToast(e instanceof Error ? e.message : String(e), 'error');
      setIsRunning(false);
      return;
    }

    setIsRunning(false);
  };

  const handleNew = () => {
    if (isRunning) return;
    setYaml(SAMPLES.hello);
    ws.reset();
  };

  const handleAgentSelect = async (agentId: string) => {
    if (isRunning) return;
    try {
      const res = await fetch(`/agents/${agentId}`);
      if (!res.ok) {
        throw new Error(`Failed to fetch agent: ${res.status}`);
      }
      const agent = await res.json();
      if (agent.yaml) {
        setYaml(agent.yaml);
      } else {
        // Fallback: Build YAML from agent object if yaml field is not available
        const yamlContent = `name: ${agent.name}\n${agent.description ? `description: ${agent.description}\n` : ''}\nsteps:\n${agent.steps.map((step: any) => {
          let stepYaml = `  - id: ${step.id}\n    name: ${step.name}\n`;
          if (step.tool) {
            stepYaml += `    tool:\n      name: ${step.tool.name}\n`;
            if (step.tool.input) {
              stepYaml += `      input: ${JSON.stringify(step.tool.input, null, 6).replace(/^/gm, '        ')}\n`;
            }
          }
          if (step.llm) {
            stepYaml += `    llm:\n      provider: ${step.llm.provider}\n      model: ${step.llm.model}\n      prompt: "${step.llm.prompt}"\n`;
            if (step.llm.max_tokens) {
              stepYaml += `      max_tokens: ${step.llm.max_tokens}\n`;
            }
          }
          if (step.depends_on) {
            stepYaml += `    depends_on:\n${step.depends_on.map((dep: string) => `      - ${dep}\n`).join('')}`;
          }
          if (step.retry) {
            stepYaml += `    retry:\n      kind: ${step.retry.kind}\n      max_retries: ${step.retry.max_retries}\n      interval_ms: ${step.retry.interval_ms}\n`;
          }
          return stepYaml;
        }).join('')}`;
        setYaml(yamlContent);
      }
    } catch (e: unknown) {
      showToast(e instanceof Error ? e.message : String(e), 'error');
    }
  };

  const handleVarsKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') void runWorkflow();
  };

  const currentStatus = ws.runStatus;
  const isActuallyRunning = currentStatus === 'running';

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
          disabled={isActuallyRunning}
          className="flex items-center gap-2 px-4 py-1.5 text-white text-sm font-medium rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          style={{ background: '#F97316' }}
        >
          {isActuallyRunning ? (
            <svg className="animate-spin" width="14" height="14" viewBox="0 0 14 14" fill="none">
              <circle cx="7" cy="7" r="6" stroke="rgba(255,255,255,0.3)" strokeWidth="2" />
              <path d="M7 1 A6 6 0 0 1 13 7" stroke="white" strokeWidth="2" strokeLinecap="round" />
            </svg>
          ) : (
            <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
              <polygon points="3,1 13,7 3,13" />
            </svg>
          )}
          {isActuallyRunning ? 'Running...' : 'Run'}
        </button>
      </header>

      {/* 3-column layout */}
      <div className="flex flex-1 min-h-0">
        <Sidebar agents={agents} onNew={handleNew} onAgentSelect={handleAgentSelect} />
        <WorkflowEditor value={yaml} onChange={setYaml} onToast={showToast} />
        <ExecutionPanel
          runStatus={ws.runStatus}
          steps={ws.steps}
          systemMessages={ws.systemMessages}
          outputs={ws.outputs}
        />
      </div>

      <ToastContainer toasts={toasts} onRemove={removeToast} />
    </div>
  );
}
