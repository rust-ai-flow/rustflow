import { useEffect, useState } from 'react';
import type { AgentSummary } from '../types';

interface SidebarProps {
  agents: AgentSummary[];
  onNew: () => void;
  onAgentSelect: (agentId: string) => void;
}

function HexLogo() {
  return (
    <svg width="28" height="28" viewBox="0 0 28 28" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path d="M14 2L25 8V20L14 26L3 20V8L14 2Z" fill="#F97316" fillOpacity="0.15" stroke="#F97316" strokeWidth="1.5" />
      <path d="M9 11L14 8L19 11V17L14 20L9 17V11Z" fill="#F97316" fillOpacity="0.5" />
      <circle cx="14" cy="14" r="2.5" fill="#F97316" />
    </svg>
  );
}

function FileIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
      <rect x="1" y="1" width="12" height="12" rx="2" stroke="#4B5563" strokeWidth="1.2" />
      <line x1="3.5" y1="4.5" x2="10.5" y2="4.5" stroke="#4B5563" strokeWidth="1" />
      <line x1="3.5" y1="7" x2="10.5" y2="7" stroke="#4B5563" strokeWidth="1" />
      <line x1="3.5" y1="9.5" x2="7.5" y2="9.5" stroke="#4B5563" strokeWidth="1" />
    </svg>
  );
}

export function Sidebar({ agents, onNew, onAgentSelect }: SidebarProps) {
  const [serverOnline, setServerOnline] = useState<boolean | null>(null);

  useEffect(() => {
    const check = async () => {
      try {
        const res = await fetch('/health', { signal: AbortSignal.timeout(3000) });
        setServerOnline(res.ok);
      } catch {
        setServerOnline(false);
      }
    };
    void check();
    const id = setInterval(() => void check(), 8000);
    return () => clearInterval(id);
  }, []);

  return (
    <aside
      className="w-60 shrink-0 flex flex-col"
      style={{ background: '#0F172A', borderRight: '1px solid #1E293B' }}
    >
      {/* Logo */}
      <div className="px-4 py-4 border-b" style={{ borderColor: '#1E293B' }}>
        <div className="flex items-center gap-2.5">
          <HexLogo />
          <div>
            <div className="text-white text-sm font-semibold leading-tight">RustFlow</div>
            <div className="text-slate-500 text-xs leading-tight">playground</div>
          </div>
        </div>
      </div>

      {/* Workflows section */}
      <div className="px-3 pt-4 pb-2 flex-1 overflow-y-auto">
        <div className="flex items-center justify-between mb-2">
          <span className="text-xs font-medium text-slate-500 uppercase tracking-wider">
            Workflows
          </span>
          <button
            onClick={onNew}
            className="text-xs text-slate-400 hover:text-white px-2 py-0.5 rounded hover:bg-slate-700 transition-colors"
          >
            + New
          </button>
        </div>

        <div className="flex flex-col gap-0.5">
          {agents.length === 0 ? (
            <div className="text-xs text-slate-600 py-2 px-2">No workflows yet</div>
          ) : (
            agents.map(agent => (
              <div
                key={agent.id}
                className="flex items-center gap-2 px-2 py-1.5 rounded cursor-pointer hover:bg-slate-800 transition-colors group"
                title={agent.description ?? agent.name}
                onClick={() => onAgentSelect(agent.id)}
              >
                <div className="mt-0.5 shrink-0">
                  <FileIcon />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="text-xs text-slate-300 truncate">{agent.name}</div>
                  <div className="text-xs text-slate-600">
                    {agent.step_count} step{agent.step_count !== 1 ? 's' : ''}
                  </div>
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      {/* Server status */}
      <div
        className="px-4 py-3 border-t flex items-center gap-2 shrink-0"
        style={{ borderColor: '#1E293B' }}
      >
        <div
          className={`w-2 h-2 rounded-full ${
            serverOnline === null
              ? 'bg-slate-600'
              : serverOnline
              ? 'bg-green-400'
              : 'bg-slate-600'
          }`}
        />
        <span className="text-xs text-slate-500">
          {serverOnline === null
            ? 'Connecting...'
            : serverOnline
            ? 'Server online'
            : 'Server offline'}
        </span>
      </div>
    </aside>
  );
}
