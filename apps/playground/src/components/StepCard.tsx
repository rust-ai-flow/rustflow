
import type { StepState } from '../types';

interface StepCardProps {
  step: StepState;
}

function formatMs(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

function SpinnerIcon({ color }: { color: string }) {
  return (
    <svg className="animate-spin" width="16" height="16" viewBox="0 0 16 16" fill="none">
      <circle cx="8" cy="8" r="7" stroke="#E2E8F0" strokeWidth="2" />
      <path d="M8 1 A7 7 0 0 1 15 8" stroke={color} strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

function StatusIcon({ status }: { status: StepState['status'] }) {
  switch (status) {
    case 'running':
      return <SpinnerIcon color="#F97316" />;
    case 'retrying':
      return <SpinnerIcon color="#F59E0B" />;
    case 'success':
      return (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="7" fill="#DCFCE7" stroke="#22C55E" strokeWidth="1.5" />
          <path d="M5 8l2 2 4-4" stroke="#16A34A" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      );
    case 'failed':
      return (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="7" fill="#FEE2E2" stroke="#EF4444" strokeWidth="1.5" />
          <path d="M5.5 5.5l5 5M10.5 5.5l-5 5" stroke="#DC2626" strokeWidth="1.5" strokeLinecap="round" />
        </svg>
      );
    default:
      return (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="7" stroke="#CBD5E1" strokeWidth="1.5" />
        </svg>
      );
  }
}

function StatusLabel({ step }: { step: StepState }) {
  switch (step.status) {
    case 'running':
      return <span className="text-xs font-medium text-orange-500">running</span>;
    case 'retrying':
      return <span className="text-xs font-medium text-amber-500">retrying #{step.attempt ?? '?'}</span>;
    case 'success':
      return <span className="text-xs font-medium text-green-600">done</span>;
    case 'failed':
      return <span className="text-xs font-medium text-red-500">failed</span>;
    default:
      return <span className="text-xs text-slate-400">pending</span>;
  }
}

function borderColor(status: StepState['status']): string {
  switch (status) {
    case 'running': return '#FED7AA';
    case 'retrying': return '#FDE68A';
    case 'success': return '#BBF7D0';
    case 'failed': return '#FECACA';
    default: return '#E2E8F0';
  }
}

export function StepCard({ step }: StepCardProps) {
  const elapsed = step.elapsed_ms != null ? formatMs(step.elapsed_ms) : '';
  const isLive = step.status === 'running' || step.status === 'retrying';

  return (
    <div
      className="bg-white border rounded-lg p-3 shadow-sm transition-all duration-200"
      style={{ borderColor: borderColor(step.status) }}
    >
      <div className="flex items-start gap-2.5">
        <div className="mt-0.5 shrink-0">
          <StatusIcon status={step.status} />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between gap-2">
            <span className="text-sm font-medium text-slate-700 truncate">{step.name}</span>
            <div className="flex items-center gap-2 shrink-0">
              <StatusLabel step={step} />
              <span className="text-xs text-slate-400 font-mono">
                {isLive && step.elapsed_ms != null ? formatMs(step.elapsed_ms) : elapsed}
              </span>
            </div>
          </div>
          <div className="text-xs text-slate-400 font-mono mt-0.5">{step.id}</div>

          {/* Success output preview */}
          {step.status === 'success' && step.output != null && (
            <div className="mt-2">
              <details className="group">
                <summary className="text-xs text-slate-400 cursor-pointer hover:text-slate-600 select-none">
                  Output
                </summary>
                <pre className="mt-1 text-xs bg-slate-50 p-2 rounded overflow-auto max-h-32 text-slate-600 font-mono leading-relaxed">
                  {JSON.stringify(step.output, null, 2)}
                </pre>
              </details>
            </div>
          )}

          {/* Error */}
          {step.status === 'failed' && step.error && (
            <div className="mt-1.5 text-xs text-red-500 bg-red-50 px-2 py-1.5 rounded font-mono leading-relaxed">
              {step.error}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
