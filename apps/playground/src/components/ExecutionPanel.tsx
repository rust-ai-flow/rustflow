import { useState } from 'react';
import type { RunStatus, StepState, SystemMessage } from '../types';
import { StepCard } from './StepCard';

interface ExecutionPanelProps {
  runStatus: RunStatus;
  steps: StepState[];
  systemMessages: SystemMessage[];
  outputs: Record<string, unknown> | null;
}

function StatusBadge({ status }: { status: RunStatus }) {
  const styles: Record<RunStatus, string> = {
    idle: 'bg-slate-100 text-slate-500',
    running: 'bg-orange-100 text-orange-600',
    completed: 'bg-green-100 text-green-700',
    failed: 'bg-red-100 text-red-600',
  };
  return (
    <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${styles[status]}`}>
      {status}
    </span>
  );
}

function EmptyState() {
  return (
    <div className="flex flex-col items-center justify-center h-full text-center py-8">
      <div className="w-12 h-12 rounded-full bg-slate-100 flex items-center justify-center mb-3">
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#94A3B8" strokeWidth="1.5">
          <circle cx="12" cy="12" r="10" />
          <polygon points="10,8 16,12 10,16" fill="#94A3B8" stroke="none" />
        </svg>
      </div>
      <p className="text-sm font-medium text-slate-400">Run a workflow</p>
      <p className="text-xs text-slate-300 mt-1">to see live results</p>
    </div>
  );
}

export function ExecutionPanel({ runStatus, steps, systemMessages, outputs }: ExecutionPanelProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    if (!outputs) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(outputs, null, 2));
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // ignore
    }
  };

  const hasContent = steps.length > 0 || systemMessages.length > 0;

  return (
    <div className="w-90 shrink-0 flex flex-col bg-white border-l border-slate-200" style={{ width: 360 }}>
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-slate-100 shrink-0">
        <span className="text-sm font-semibold text-slate-700">Execution</span>
        <StatusBadge status={runStatus} />
      </div>

      {/* Steps area */}
      <div className="flex-1 overflow-y-auto p-4">
        {!hasContent ? (
          <EmptyState />
        ) : (
          <>
            <div className="flex flex-col gap-2">
              {steps.map(step => (
                <StepCard key={step.id} step={step} />
              ))}
            </div>

            {systemMessages.length > 0 && (
              <div className="flex flex-col gap-1 mt-2">
                {systemMessages.map(msg => (
                  <div
                    key={msg.id}
                    className={`text-xs px-3 py-2 rounded border ${
                      msg.type === 'warning'
                        ? 'text-amber-600 bg-amber-50 border-amber-200'
                        : 'text-slate-500 bg-slate-50 border-slate-200'
                    }`}
                  >
                    {msg.text}
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>

      {/* Output section */}
      {outputs && (
        <div className="border-t border-slate-100 px-4 py-3 shrink-0">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold text-slate-600 uppercase tracking-wide">Output</span>
            <button
              onClick={() => void handleCopy()}
              className="text-xs text-slate-400 hover:text-slate-600 transition-colors flex items-center gap-1"
            >
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.2">
                <rect x="4" y="4" width="7" height="7" rx="1" />
                <path d="M8 4V2a1 1 0 0 0-1-1H2a1 1 0 0 0-1 1v5a1 1 0 0 0 1 1h2" />
              </svg>
              {copied ? 'Copied!' : 'Copy'}
            </button>
          </div>
          <pre
            className="font-mono text-xs overflow-auto max-h-44 p-3 rounded leading-relaxed"
            style={{ background: '#0D1117', color: '#E2E8F0' }}
          >
            {JSON.stringify(outputs, null, 2)}
          </pre>
        </div>
      )}
    </div>
  );
}
