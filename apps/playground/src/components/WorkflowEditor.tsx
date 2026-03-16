import React, { useCallback, useEffect, useRef, useState } from 'react';
import jsyaml from 'js-yaml';

const SAMPLES: Record<string, string> = {
  hello: `name: hello-agent
description: >
  A minimal example agent that searches the web for a topic,
  summarises the results with an LLM, then formats the summary
  as a structured report.

steps:
  - id: search
    name: Web Search
    tool:
      name: http
      input:
        url: "https://en.wikipedia.org/api/rest_v1/page/summary/Rust_(programming_language)"
        method: GET
        headers:
          Accept: application/json
    retry:
      kind: fixed
      max_retries: 3
      interval_ms: 1000
    timeout_ms: 10000

  - id: summarize
    name: Summarize Results
    llm:
      provider: glm
      model: glm-4.5-air
      prompt: |
        Here is a Wikipedia extract about the Rust programming language:

        {{steps.search.output}}

        Please write a concise 3-paragraph summary covering:
        1. What Rust is and its primary goals
        2. Key features (ownership, borrowing, lifetimes)
        3. Where Rust is commonly used today
      temperature: 0.3
      max_tokens: 1000
    depends_on:
      - search
    timeout_ms: 30000

  - id: format
    name: Format Report
    llm:
      provider: glm
      model: glm-4.5-air
      prompt: |
        Convert the following summary into a Markdown report with:
        - A top-level heading
        - Numbered sections matching the 3 paragraphs
        - A "Key Takeaways" bullet list at the end

        Summary:
        {{steps.summarize.output}}
      temperature: 0.1
      max_tokens: 1000
    depends_on:
      - summarize
    timeout_ms: 30000`,

  http: `name: http-example
description: >
  A simple workflow that uses only the HTTP tool.
  No LLM API key required - great for testing!

steps:
  - id: fetch-ip
    name: Fetch Public IP
    tool:
      name: http
      input:
        url: "https://httpbin.org/ip"
        method: GET

  - id: fetch-headers
    name: Fetch Request Headers
    tool:
      name: http
      input:
        url: "https://httpbin.org/headers"
        method: GET`,

  complex: `name: complex-ollama-workflow
description: Complex workflow example using public HTTP APIs and remote Ollama models

steps:
  # Step 1: Fetch sample JSON data
  - id: fetch_sample_data
    name: Fetch Sample JSON Data
    tool:
      name: http
      input:
        url: "https://httpbin.org/json"
        method: GET
    retry:
      kind: fixed
      max_retries: 2
      interval_ms: 1000

  # Step 2: Fetch random user data
  - id: fetch_random_user
    name: Fetch Random User Data
    tool:
      name: http
      input:
        url: "https://randomuser.me/api/"
        method: GET
    retry:
      kind: fixed
      max_retries: 2
      interval_ms: 1000

  # Step 3: Fetch cat fact
  - id: fetch_cat_fact
    name: Fetch Cat Fact
    tool:
      name: http
      input:
        url: "https://catfact.ninja/fact"
        method: GET
    retry:
      kind: fixed
      max_retries: 2
      interval_ms: 1000

  # Step 4: Analyze sample data
  - id: analyze_sample_data
    name: Analyze Sample Data
    llm:
      provider: glm
      model: glm-4.5-air
      prompt: "Please analyze the following JSON data, extract key information and provide a detailed interpretation:\n{{steps.fetch_sample_data.output}}"
      max_tokens: 50000
      temperature: 0.7
    depends_on:
      - fetch_sample_data
    timeout_ms: 45000

  # Step 5: Analyze user data
  - id: analyze_user
    name: Analyze User Data
    llm:
      provider: glm
      model: glm-4.5-air
      prompt: "Please analyze the following random user data, extract detailed information including name, gender, age, address, etc., and generate a short user profile:\n{{steps.fetch_random_user.output}}"
      max_tokens: 50000
      temperature: 0.7
    depends_on:
      - fetch_random_user
    timeout_ms: 45000

  # Step 6: Analyze cat fact
  - id: analyze_cat_fact
    name: Analyze Cat Fact
    llm:
      provider: glm
      model: glm-4.5-air
      prompt: "Please analyze the following cat fact, expand on it and provide more related interesting information:\n{{steps.fetch_cat_fact.output}}"
      max_tokens: 50000
      temperature: 0.7
    depends_on:
      - fetch_cat_fact
    timeout_ms: 45000

  # Step 7: Generate comprehensive report
  - id: generate_report
    name: Generate Comprehensive Report
    llm:
      provider: glm
      model: glm-4.5-air
      prompt: "Based on the following three analysis results, generate an interesting comprehensive report including data interpretation, user analysis, and cat facts:\n\nSample Data Analysis:\n{{steps.analyze_sample_data.output}}\n\nUser Data Analysis:\n{{steps.analyze_user.output}}\n\nCat Fact Analysis:\n{{steps.analyze_cat_fact.output}}"
      max_tokens: 50000
      temperature: 0.5
    depends_on:
      - analyze_sample_data
      - analyze_user
      - analyze_cat_fact
    timeout_ms: 60000

  # Step 8: Generate structured data
  - id: generate_structured_data
    name: Generate Structured Data
    llm:
      provider: glm
      model: glm-4.5-air
      prompt: "Please convert the following comprehensive report into structured JSON format, including four main sections: sample_data (sample data analysis), user (user data analysis), cat_fact (cat fact analysis), and summary (summary):\n{{steps.generate_report.output}}\n\nPlease output only JSON format, no additional text."
      max_tokens: 50000
      temperature: 0.3
    depends_on:
      - generate_report
    timeout_ms: 45000`,

  all: `name: all-tools-workflow
description: Comprehensive example demonstrating all built-in tools

steps:
  # Step 1: Read environment variable
  - id: read_env
    name: Read Environment Variable
    tool:
      name: env
      input:
        name: "HOME"
    retry:
      kind: fixed
      max_retries: 2
      interval_ms: 500

  # Step 2: Fetch data using HTTP tool
  - id: fetch_data
    name: Fetch Sample Data
    tool:
      name: http
      input:
        url: "https://jsonplaceholder.typicode.com/posts/1"
        method: GET
    retry:
      kind: fixed
      max_retries: 3
      interval_ms: 1000

  # Step 3: Write data to file
  - id: write_data
    name: Write Data to File
    tool:
      name: file_write
      input:
        path: "/tmp/rustflow_example_data.json"
        content: "{{steps.fetch_data.output}}"
    depends_on:
      - fetch_data

  # Step 4: Execute shell command to check file
  - id: check_file
    name: Check File Content
    tool:
      name: shell
      input:
        command: "cat /tmp/rustflow_example_data.json | jq '.title'"
        timeout_secs: 10
    depends_on:
      - write_data

  # Step 5: Read file content
  - id: read_file
    name: Read File Content
    tool:
      name: file_read
      input:
        path: "/tmp/rustflow_example_data.json"
    depends_on:
      - write_data

  # Step 6: Analyze data using LLM
  - id: analyze_data
    name: Analyze Data
    llm:
      provider: glm
      model: glm-4.5-air
      prompt: "Please analyze the following JSON data, extract key information and generate a concise summary:\n{{steps.read_file.output}}"
      max_tokens: 500
      temperature: 0.3
    depends_on:
      - read_file

  # Step 7: Write analysis result to file
  - id: write_analysis
    name: Write Analysis Result
    tool:
      name: file_write
      input:
        path: "/tmp/rustflow_analysis.txt"
        content: "{{steps.analyze_data.output}}"
    depends_on:
      - analyze_data

  # Step 8: Sleep for a while
  - id: sleep
    name: Wait 2 Seconds
    tool:
      name: sleep
      input:
        ms: 2000
    depends_on:
      - write_analysis

  # Step 9: Final check
  - id: final_check
    name: Final Check
    tool:
      name: shell
      input:
        command: "ls -la /tmp/rustflow_*.{json,txt}"
        timeout_secs: 10
    depends_on:
      - sleep`,
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
                {
                  [
                    { key: 'hello', label: 'hello-agent', sub: 'Minimal example agent with web search and LLM' },
                    { key: 'http', label: 'http-example', sub: 'Simple workflow using only HTTP tool' },
                    { key: 'complex', label: 'complex-ollama-workflow', sub: 'Complex workflow with multiple API calls and LLM analysis' },
                    { key: 'all', label: 'all-tools-workflow', sub: 'Comprehensive example demonstrating all built-in tools' },
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
