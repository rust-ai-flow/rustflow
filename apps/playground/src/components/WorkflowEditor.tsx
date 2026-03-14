import React, { useCallback, useEffect, useRef, useState } from 'react';
import jsyaml from 'js-yaml';

const SAMPLES: Record<string, string> = {
  hello: `name: hello-playground
description: A sample parallel workflow

steps:
  - id: step_a
    name: Fetch Config
    tool:
      name: sleep
      input:
        ms: 800

  - id: step_b
    name: Load Data
    tool:
      name: sleep
      input:
        ms: 600

  - id: step_c
    name: Process Results
    tool:
      name: sleep
      input:
        ms: 400
    depends_on:
      - step_a
      - step_b`,

  env: `name: env-check
description: Check environment variables

steps:
  - id: check_env
    name: Read Environment
    tool:
      name: env
      input:
        key: PATH`,

  http: `name: http-request
description: Fetch data via HTTP

steps:
  - id: fetch
    name: Fetch JSON
    tool:
      name: http
      input:
        url: "https://httpbin.org/json"
        method: GET

  - id: extract
    name: Extract Data
    tool:
      name: json_extract
      input:
        path: "slideshow.title"
    depends_on:
      - fetch`,
};

interface WorkflowEditorProps {
  value: string;
  onChange: (value: string) => void;
  onToast: (msg: string, type?: 'info' | 'success' | 'error') => void;
}

export function WorkflowEditor({ value, onChange, onToast }: WorkflowEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const lineNumbersRef = useRef<HTMLDivElement>(null);
  const [sampleMenuOpen, setSampleMenuOpen] = useState(false);
  const [cursorPos, setCursorPos] = useState({ ln: 1, col: 1 });

  const updateLineNumbers = useCallback(() => {
    if (!lineNumbersRef.current || !textareaRef.current) return;
    const lines = textareaRef.current.value.split('\n');
    lineNumbersRef.current.textContent = lines.map((_: string, i: number) => i + 1).join('\n');
  }, []);

  const syncScroll = useCallback(() => {
    if (!lineNumbersRef.current || !textareaRef.current) return;
    lineNumbersRef.current.scrollTop = textareaRef.current.scrollTop;
  }, []);

  const updateCursorPos = useCallback(() => {
    if (!textareaRef.current) return;
    const text = textareaRef.current.value.substring(0, textareaRef.current.selectionStart);
    const lines = text.split('\n');
    setCursorPos({ ln: lines.length, col: lines[lines.length - 1].length + 1 });
  }, []);

  useEffect(() => {
    updateLineNumbers();
  }, [value, updateLineNumbers]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Tab') {
      e.preventDefault();
      const ta = e.currentTarget;
      const start = ta.selectionStart;
      const end = ta.selectionEnd;
      const newVal = ta.value.substring(0, start) + '  ' + ta.value.substring(end);
      onChange(newVal);
      setTimeout(() => {
        if (textareaRef.current) {
          textareaRef.current.selectionStart = start + 2;
          textareaRef.current.selectionEnd = start + 2;
        }
      }, 0);
    }
    updateCursorPos();
  };

  const handleFormat = () => {
    try {
      const doc = jsyaml.load(value);
      if (doc) {
        onChange(jsyaml.dump(doc, { indent: 2, lineWidth: 100 }));
        onToast('Formatted', 'success');
      }
    } catch (e: unknown) {
      onToast('YAML error: ' + (e instanceof Error ? e.message : String(e)), 'error');
    }
  };

  const loadSample = (key: string) => {
    onChange(SAMPLES[key] ?? SAMPLES.hello);
    setSampleMenuOpen(false);
    onToast('Sample loaded', 'info');
  };

  return (
    <div className="flex flex-col flex-1 min-w-0" style={{ background: '#0D1117' }}>
      {/* Tab bar */}
      <div
        className="flex items-center px-3 py-1.5 border-b shrink-0"
        style={{ background: '#161B22', borderColor: '#30363D' }}
      >
        <div
          className="flex items-center gap-1.5 px-3 py-1 rounded text-sm font-mono text-slate-300"
          style={{ background: '#0D1117', border: '1px solid #30363D' }}
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
            <rect x="1" y="1" width="10" height="10" rx="1" stroke="#6B7280" strokeWidth="1" />
            <line x1="3" y1="4" x2="9" y2="4" stroke="#6B7280" strokeWidth="1" />
            <line x1="3" y1="6" x2="9" y2="6" stroke="#6B7280" strokeWidth="1" />
            <line x1="3" y1="8" x2="7" y2="8" stroke="#6B7280" strokeWidth="1" />
          </svg>
          workflow.yaml
        </div>

        <div className="flex-1" />

        <div className="flex items-center gap-2">
          <button
            onClick={handleFormat}
            className="text-xs text-slate-400 hover:text-slate-200 px-2 py-1 rounded hover:bg-slate-700 transition-colors font-mono"
          >
            Format
          </button>

          <div className="relative">
            <button
              onClick={() => setSampleMenuOpen(v => !v)}
              className="text-xs text-slate-400 hover:text-slate-200 px-2 py-1 rounded hover:bg-slate-700 transition-colors font-mono flex items-center gap-1"
            >
              Sample
              <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor">
                <path d="M2 4l3 3 3-3" />
              </svg>
            </button>

            {sampleMenuOpen && (
              <div
                className="absolute top-full right-0 z-50 border rounded-md min-w-48 shadow-xl"
                style={{ background: '#1E293B', borderColor: '#374151' }}
              >
                {[
                  { key: 'hello', label: 'hello-playground', sub: 'Parallel sleep workflow' },
                  { key: 'env', label: 'env-check', sub: 'Read environment variables' },
                  { key: 'http', label: 'http-request', sub: 'Fetch data via HTTP' },
                ].map(item => (
                  <div
                    key={item.key}
                    onClick={() => loadSample(item.key)}
                    className="px-3 py-2.5 cursor-pointer hover:bg-slate-700 transition-colors"
                    style={{ color: '#CBD5E1' }}
                  >
                    <div className="text-sm font-medium">{item.label}</div>
                    <div className="text-xs mt-0.5" style={{ color: '#6B7280' }}>{item.sub}</div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Editor with line numbers */}
      <div className="relative flex flex-1 overflow-hidden" style={{ background: '#0D1117' }}>
        <div
          ref={lineNumbersRef}
          className="font-mono text-right select-none overflow-hidden shrink-0"
          style={{
            padding: '16px 12px 16px 8px',
            color: '#4B5563',
            fontSize: 13,
            lineHeight: '21px',
            minWidth: 48,
            borderRight: '1px solid #1F2937',
            whiteSpace: 'pre',
          }}
        >
          1
        </div>
        <textarea
          ref={textareaRef}
          value={value}
          onChange={e => { onChange(e.target.value); updateCursorPos(); }}
          onKeyDown={handleKeyDown}
          onScroll={syncScroll}
          onClick={updateCursorPos}
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          className="flex-1 resize-none outline-none border-none"
          style={{
            padding: 16,
            background: 'transparent',
            color: '#E2E8F0',
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 13,
            lineHeight: '21px',
            caretColor: '#F97316',
            tabSize: 2,
            whiteSpace: 'pre',
            overflowY: 'auto',
          }}
        />
      </div>

      {/* Status bar */}
      <div
        className="flex items-center justify-between px-4 py-1 text-xs border-t shrink-0"
        style={{ background: '#161B22', borderColor: '#30363D', color: '#6B7280' }}
      >
        <span>YAML</span>
        <span>Ln {cursorPos.ln}, Col {cursorPos.col}</span>
      </div>
    </div>
  );
}

export { SAMPLES };
