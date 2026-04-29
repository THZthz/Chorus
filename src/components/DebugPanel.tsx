import React, { useState, useEffect, useCallback } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import {
  Terminal, X, ChevronDown, ChevronRight, RefreshCw, Trash2, Bug,
  GripHorizontal, Copy, Check, MessageSquare, Database, Save, Plus, AlertCircle, Monitor, WrapText,
  Search, Filter, Calendar, Clock
} from 'lucide-react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import { EditorView } from '@codemirror/view';
import { jsonSchema } from 'codemirror-json-schema';
import { Message } from '@/types/dialogue';
import { WorldState, WorldEntity } from '@/types/entities';
import { consoleLogger, ConsoleLog, LogLevel } from '@/services/ConsoleLogger';

interface LlmLog {
  id: string;
  timestamp: string;
  request: string;
  response: string | null;
  duration: number | null;
  status: string;
}

const ResizableContainer: React.FC<{
  children: React.ReactNode;
  defaultHeight?: number;
  className?: string;
}> = ({ children, defaultHeight = 150, className }) => {
  const [height, setHeight] = useState(defaultHeight);
  const [isResizing, setIsResizing] = useState(false);

  const startResizing = useCallback((mouseDownEvent: React.MouseEvent) => {
    setIsResizing(true);
    const startY = mouseDownEvent.clientY;
    const startHeight = height;

    const onMouseMove = (mouseMoveEvent: MouseEvent) => {
      const newHeight = startHeight + mouseMoveEvent.clientY - startY;
      setHeight(Math.max(80, newHeight));
    };

    const onMouseUp = () => {
      setIsResizing(false);
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    };

    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }, [height]);

  return (
    <div
      className={`relative rounded-sm border border-white/5 bg-[#0b0c0e] flex flex-col group/resizable overflow-hidden ${className}`}
      style={{ height: `${height}px` }}
    >
      <div className="flex-1 min-h-0 relative">
        {children}
      </div>
      <div
        onMouseDown={startResizing}
        className={`h-2 w-full flex items-center justify-center cursor-ns-resize hover:bg-white/5 active:bg-white/10 transition-colors border-t border-white/5 flex-shrink-0 relative z-10 ${isResizing ? 'bg-white/10' : ''}`}
      >
        <div className={`w-12 h-[1px] transition-colors ${isResizing ? 'bg-white/40' : 'bg-white/10 group-hover/resizable:bg-white/20'}`} />
      </div>
    </div>
  );
};

const MESSAGE_SCHEMA = {
  type: 'array',
  items: {
    type: 'object',
    properties: {
      id: { type: 'string', description: 'Unique identifier for the message' },
      speaker: { type: 'string', description: 'Name of the entity speaking' },
      type: {
        enum: ['YOU', 'INNER_VOICE', 'CHARACTER', 'SYSTEM', 'ROLL', 'NOTIFICATION'],
        description: 'Style/category of the message'
      },
      text: { type: 'string', description: 'Content of the message' },
      metadata: {
        type: 'object',
        properties: {
          notificationType: { enum: ['XP', 'TASK', 'ITEM'] }
        }
      }
    },
    required: ['id', 'speaker', 'type', 'text']
  }
};

const ENTITY_SCHEMA = {
  type: 'object',
  properties: {
    id: { type: 'string', description: 'Unique identifier for the entity' },
    type: { enum: ['OBJECT', 'LOCATION', 'CHARACTER'], description: 'Category of world entity' },
    displayName: { type: 'string', description: 'Friendly name shown in UI' },
    shortDescription: { type: 'string' },
    longDescription: { type: 'string' },
    attributes: { type: 'object', description: 'Additional dynamic properties' },
    stats: { type: 'object', description: 'Numeric character attributes (only for CHARACTER)' },
    opinions: { type: 'object', description: 'Relationship mapping (only for CHARACTER)' }
  },
  required: ['id', 'type', 'displayName']
};

const debugTheme = EditorView.theme({
  "&": {
    fontSize: "11px",
    backgroundColor: "#0d0d0d !important",
  },
  "&.cm-focused": {
    outline: "none",
  },
  ".cm-content": {
    fontFamily: "var(--font-mono)",
  },
  ".cm-gutters": {
    backgroundColor: "#0d0d0d !important",
    borderRight: "1px solid rgba(255, 255, 255, 0.05)",
    color: "#4b5563 !important",
    minWidth: "32px",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "rgba(255, 255, 255, 0.05) !important",
    color: "#9ca3af !important",
  },
  ".cm-activeLine": {
    backgroundColor: "rgba(255, 255, 255, 0.02) !important",
  },
  ".cm-scroller::-webkit-scrollbar": {
    width: "4px",
    height: "4px",
  },
  ".cm-scroller::-webkit-scrollbar-track": {
    background: "transparent",
  },
  ".cm-scroller::-webkit-scrollbar-thumb": {
    background: "rgba(255, 255, 255, 0.05)",
    borderRadius: "0px",
  },
  ".cm-scroller::-webkit-scrollbar-thumb:hover": {
    background: "rgba(255, 255, 255, 0.15)",
  },
  ".cm-scroller": {
    scrollbarWidth: "thin",
    scrollbarColor: "rgba(255, 255, 255, 0.05) transparent",
  },
  ".cm-selectionBackground": {
    backgroundColor: "rgba(255, 255, 255, 0.1) !important",
  },
  ".cm-tooltip": {
    border: "1px solid rgba(255, 255, 255, 0.1) !important",
    backgroundColor: "#1a1a1a !important",
    backdropFilter: "blur(4px)",
    borderRadius: "2px",
    boxShadow: "0 10px 15px -3px rgba(0, 0, 0, 0.5)",
    padding: "4px",
    color: "#e2e8f0",
  },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
    backgroundColor: "rgba(255, 255, 255, 0.1) !important",
    color: "#ffffff !important",
    borderRadius: "2px",
  },
  ".cm-json-schema-tooltip-title": {
    color: "#9081e3",
    fontWeight: "bold",
    marginBottom: "4px",
    fontSize: "10px",
    textTransform: "uppercase",
    letterSpacing: "0.1em",
  },
}, { dark: true });

const JsonNode: React.FC<{
  label?: string;
  value: any;
  depth: number;
  isLast?: boolean;
  isWrapping?: boolean;
}> = ({ label, value, depth, isLast = true, isWrapping = true }) => {
  const [isExpanded, setIsExpanded] = useState(depth < 1);
  const hasChildren = value !== null && typeof value === 'object';
  const isEmpty = hasChildren && (Array.isArray(value) ? value.length === 0 : Object.keys(value).length === 0);

  const renderValue = () => {
    if (value === null) return <span className="text-[#5c6370]">null</span>;
    if (typeof value === 'string') {
      return (
        <span className={`text-[#98c379] ${isWrapping ? 'whitespace-pre-wrap break-words' : 'whitespace-pre'}`}>
          "{value}"
        </span>
      );
    }
    if (typeof value === 'number') return <span className="text-[#d19a66]">{value}</span>;
    if (typeof value === 'boolean') return <span className="text-[#c678dd] font-bold">{value.toString()}</span>;

    if (Array.isArray(value)) {
      if (isEmpty) return <span className="text-[#abb2bf]/20">[]</span>;

      return isExpanded ? (
        <span>
          <span className="text-[#abb2bf]/70">[</span>
          <div className="pl-4 border-l border-white/[0.03] ml-1.5 my-0.5">
            {value.map((v, i) => (
              <JsonNode key={i} value={v} depth={depth + 1} isLast={i === value.length - 1} isWrapping={isWrapping} />
            ))}
          </div>
          <span className="text-[#abb2bf]/70">]</span>
        </span>
      ) : (
        <button
          onClick={() => setIsExpanded(true)}
          className="text-[#5c6370] hover:text-[#61afef] bg-white/[0.03] px-1 rounded transition-colors text-[10px]"
        >
          [{value.length} items]
        </button>
      );
    }

    // Object
    if (isEmpty) return <span className="text-[#abb2bf]/20">{"{}"}</span>;

    return isExpanded ? (
      <span>
        <span className="text-[#abb2bf]/70">{"{"}</span>
        <div className="pl-4 border-l border-white/[0.03] ml-1.5 my-0.5">
          {Object.entries(value).map(([k, v], i, arr) => (
            <JsonNode key={k} label={k} value={v} depth={depth + 1} isLast={i === arr.length - 1} isWrapping={isWrapping} />
          ))}
        </div>
        <span className="text-[#abb2bf]/70">{"}"}</span>
      </span>
    ) : (
      <button
        onClick={() => setIsExpanded(true)}
        className="text-[#5c6370] hover:text-[#61afef] bg-white/[0.03] px-1 rounded transition-colors text-[10px]"
      >
        {"{ " + Object.keys(value).slice(0, 2).join(', ') + (Object.keys(value).length > 2 ? '...' : '') + " }"}
      </button>
    );
  };

  return (
    <div className="font-mono text-[11px] leading-relaxed group/node">
      <div className="flex items-start">
        <div className="w-4 h-4 flex items-center justify-center flex-shrink-0 mt-0.5">
          {hasChildren && !isEmpty && (
            <button
              onClick={() => setIsExpanded(!isExpanded)}
              className={`text-gray-500 hover:text-blue-400 transition-transform duration-200 ${isExpanded ? 'rotate-0' : '-rotate-90'}`}
            >
              <ChevronDown size={12} />
            </button>
          )}
        </div>

        <div className="flex-1 min-w-0">
          {label && (
            <span className="text-[#e06c75] mr-2 group-hover/node:text-[#e06c75] transition-colors">
              <span className="text-[#abb2bf]">"</span>{label}<span className="text-[#abb2bf]">"</span>
              <span className="text-[#abb2bf] ml-1">:</span>
            </span>
          )}
          {renderValue()}
          {!isLast && <span className="text-[#abb2bf]">,</span>}
        </div>
      </div>
    </div>
  );
};

const JsonExplorer: React.FC<{ data: string | null; isWrapping?: boolean }> = ({ data, isWrapping = true }) => {
  const [parsed, setParsed] = useState<any>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!data) return;
    try {
      setParsed(JSON.parse(data));
      setError(null);
    } catch (e) {
      setError(data);
    }
  }, [data]);

  if (!data) return <div className="p-4 text-white/30 italic text-[10px] uppercase tracking-widest">Empty_Transmission</div>;

  if (error) {
    return (
      <div className="p-3 h-full overflow-auto debug-scrollbar bg-transparent">
        <pre className={`text-[#e06c75] text-[11px] font-mono leading-relaxed ${isWrapping ? 'whitespace-pre-wrap' : 'whitespace-pre'}`}>
          {error}
        </pre>
      </div>
    );
  }

  if (!parsed) return null;

  return (
    <div className="p-3 h-full overflow-auto debug-scrollbar bg-transparent">
      <div className={!isWrapping ? "w-fit min-w-full" : ""}>
        <JsonNode value={parsed} depth={0} isWrapping={isWrapping} />
      </div>
    </div>
  );
};

const CopyButton: React.FC<{ content: string }> = ({ content }) => {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy!', err);
    }
  };

  return (
    <button
      onClick={handleCopy}
      className={`p-1 flex items-center gap-1.5 transition-all ${copied
        ? 'text-[#a3c2a3]'
        : 'text-white/40 hover:text-white'
        }`}
      title={copied ? "Copied!" : "Copy to clipboard"}
    >
      {copied ? <Check size={12} /> : <Copy size={12} />}
      <span className="text-[10px] font-bold uppercase tracking-wider">
        {copied ? 'Copied' : 'Copy'}
      </span>
    </button>
  );
};

const LlmTraceViewer: React.FC = () => {
  const [logs, setLogs] = useState<LlmLog[]>([]);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isWrapping, setIsWrapping] = useState(true);

  const fetchLogs = async () => {
    setIsLoading(true);
    try {
      const response = await fetch('/api/debug/logs');
      if (response.ok) {
        const data = await response.json();
        setLogs(data);
      }
    } catch (error) {
      console.error("Failed to fetch logs:", error);
    } finally {
      setIsLoading(false);
    }
  };

  const clearLogs = async () => {
    try {
      await fetch('/api/debug/logs/clear', { method: 'POST' });
      setLogs([]);
    } catch (error) {
      console.error("Failed to clear logs:", error);
    }
  };

  useEffect(() => {
    fetchLogs();
  }, []);

  const formatJson = (jsonStr: string | null) => {
    if (!jsonStr) return 'N/A';
    try {
      return JSON.stringify(JSON.parse(jsonStr), null, 2);
    } catch (e) {
      return jsonStr;
    }
  };

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="flex items-center justify-between h-9 mb-6 flex-shrink-0">
        <div className="flex items-center gap-2 text-white/60">
          <Terminal size={16} />
          <h3 className="text-[10px] font-bold uppercase tracking-[0.2em]">LLM_TRACE</h3>
        </div>
        <div className="flex items-center gap-3">
          <button
            onClick={() => setIsWrapping(!isWrapping)}
            className={`flex items-center gap-2 px-3 py-1 rounded-sm border transition-all ${
              isWrapping
                ? 'bg-white/10 text-white border-white/20'
                : 'bg-white/5 text-white/40 border-white/10 hover:bg-white/10 hover:text-white/60'
            }`}
            title={isWrapping ? "Disable text wrap" : "Enable text wrap"}
          >
            <WrapText size={14} className={isWrapping ? 'text-blue-400' : ''} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Wrap</span>
          </button>
          <button
            onClick={fetchLogs}
            disabled={isLoading}
            className="flex items-center gap-2 px-3 py-1 bg-white/5 text-white/60 hover:bg-white/10 hover:text-white rounded-sm border border-white/10 transition-all disabled:opacity-50"
          >
            <RefreshCw size={14} className={isLoading ? 'animate-spin' : ''} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Refresh</span>
          </button>
          <button
            onClick={clearLogs}
            className="flex items-center gap-2 px-3 py-1 bg-white/5 text-white/40 hover:bg-red-500/20 hover:text-red-400 rounded-sm border border-white/5 hover:border-red-500/20 transition-all"
          >
            <Trash2 size={14} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Clear</span>
          </button>
        </div>
      </div>
      <div className="flex-1 space-y-4 overflow-y-auto debug-scrollbar pr-1">
        {logs.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-white/10 py-20 grayscale opacity-50">
            <Bug size={48} className="mb-4" />
            <p className="uppercase tracking-[0.3em] text-[10px] font-bold">Awaiting_Transmission...</p>
          </div>
        ) : (
          <div className="space-y-4">
            {logs.map((log) => (
              <div
                key={log.id}
                className={`border rounded-sm overflow-hidden transition-all duration-300 ${expandedId === log.id
                  ? 'border-white/[0.15] bg-white/[0.03]'
                  : 'border-white/5 hover:border-white/[0.08] bg-white/[0.015]'
                  }`}
              >
                <button
                  onClick={() => setExpandedId(expandedId === log.id ? null : log.id)}
                  className="w-full text-left p-4 flex items-center justify-between gap-4 group"
                >
                  <div className="flex items-center gap-4 min-w-0">
                    <div className={`transition-transform duration-300 ${expandedId === log.id ? 'rotate-0' : '-rotate-90 opacity-40'}`}>
                      <ChevronDown size={14} />
                    </div>
                    <span className={`px-2 py-0.5 rounded-sm text-[9px] font-bold tracking-widest uppercase border ${log.status === 'ERROR' ? 'bg-red-500/5 text-red-400 border-red-500/20' : 'bg-white/5 text-white/60 border-white/10'
                      }`}>
                      {log.status}
                    </span>
                    <span className="text-white/30 font-mono text-[10px] tabular-nums tracking-widest whitespace-nowrap">
                      {new Date(log.timestamp).toLocaleString([], { 
                        year: '2-digit',
                        month: '2-digit', 
                        day: '2-digit', 
                        hour: '2-digit', 
                        minute: '2-digit', 
                        second: '2-digit',
                        hour12: false 
                      }).replace(',', '')}
                    </span>
                  </div>
                  <div className="flex items-center gap-2 font-mono text-[10px] opacity-40 group-hover:opacity-100 transition-opacity">
                    <span className="tracking-widest">
                      {log.duration}ms
                    </span>
                  </div>
                </button>

                <AnimatePresence>
                  {expandedId === log.id && (
                    <motion.div
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{ duration: 0.3 }}
                      className="overflow-hidden border-t border-white/10"
                    >
                      <div className="divide-y divide-white/[0.03]">
                        <div className="p-5 bg-[#0b0c0e]">
                          <div className="flex justify-between items-center mb-4">
                            <h3 className="text-[10px] font-bold text-white/20 uppercase tracking-[0.2em] flex items-center gap-3">
                              <div className="w-[1px] h-3 bg-[#e06c75]" />
                              Outgoing_Request
                            </h3>
                            <CopyButton content={formatJson(log.request)} />
                          </div>
                          <ResizableContainer>
                            <JsonExplorer data={log.request} isWrapping={isWrapping} />
                          </ResizableContainer>
                        </div>

                        {(() => {
                          let steps: any[] = [];
                          try {
                            const parsed = typeof log.response === 'string' ? JSON.parse(log.response || "{}") : log.response;
                            steps = parsed?.steps || [];
                          } catch (e) {}

                          if (steps.length === 0) return null;

                          return (
                            <div className="p-5 bg-[#0f1013]">
                              <h3 className="text-[10px] font-bold text-white/20 uppercase tracking-[0.2em] flex items-center gap-3 mb-4">
                                <div className="w-[1px] h-3 bg-[#eab308]" />
                                GM_Assistant_Communication
                              </h3>
                              <div className="space-y-4 font-mono text-[11px]">
                                {steps.map((step: any, stepIndex: number) => {
                                  const contents = step.content || [];
                                  const textBlocks = contents.filter((c: any) => c.type === 'text');
                                  const toolCalls = contents.filter((c: any) => c.type === 'tool-call');
                                  const toolResults = contents.filter((c: any) => c.type === 'tool-result');

                                  return (
                                    <div key={stepIndex} className="space-y-3">
                                      {textBlocks.map((block: any, i: number) => (
                                        block.text ? (
                                          <div key={`text-${i}`} className="flex gap-4">
                                            <div className="w-24 flex-shrink-0 text-white/30 text-right font-bold tracking-wider uppercase">GM (Thinks)</div>
                                            <div className="flex-1 text-white/50 italic whitespace-pre-wrap break-words">{block.text}</div>
                                          </div>
                                        ) : null
                                      ))}
                                      {toolCalls.map((call: any, callIndex: number) => {
                                        const result = toolResults.find((r: any) => r.toolCallId === call.toolCallId);
                                        const isComm = call.toolName === 'communicateAssistant';
                                        
                                        // The args could be in call.input or call.args
                                        const args = call.args || call.input || {};
                                        // The out could be in result.output or result.result
                                        const out = result ? (result.output ?? result.result) : null;

                                        return (
                                          <div key={`call-${callIndex}`} className={`flex flex-col gap-2 p-3 border rounded-sm ${
                                            isComm ? 'bg-[#61afef]/5 border-[#61afef]/20' : 'bg-white/[0.02] border-white/5'
                                          }`}>
                                            <div className="flex gap-4">
                                              <div className={`w-24 flex-shrink-0 text-right tracking-wider uppercase ${isComm ? 'text-[#61afef] font-bold' : 'text-white/30'}`}>
                                                GM ({call.toolName})
                                              </div>
                                              <div className={`flex-1 overflow-auto debug-scrollbar ${isComm ? 'text-[#61afef]/90 font-bold whitespace-pre-wrap break-words' : 'text-white/40'}`}>
                                                {isComm ? args?.message : JSON.stringify(args)}
                                              </div>
                                            </div>
                                            {out && (
                                              <div className="flex gap-4 border-t border-white/5 pt-2 mt-1">
                                                <div className={`w-24 flex-shrink-0 text-right tracking-wider uppercase ${isComm ? 'text-[#c678dd] font-bold' : 'text-white/30'}`}>
                                                  {isComm ? 'Assistant' : 'System'}
                                                </div>
                                                <div className={`flex-1 whitespace-pre-wrap break-words ${
                                                  isComm && typeof out === 'string' && out.includes('SUCCESS')
                                                    ? 'text-[#98c379]'
                                                    : isComm && typeof out === 'string' && out.includes('FEEDBACK')
                                                      ? 'text-[#e06c75]'
                                                      : 'text-white/40'
                                                }`}>
                                                  {typeof out === 'string' ? out : (out.value ? out.value : JSON.stringify(out))}
                                                </div>
                                              </div>
                                            )}
                                          </div>
                                        );
                                      })}
                                    </div>
                                  );
                                })}
                              </div>
                            </div>
                          );
                        })()}

                        <div className="p-5 bg-[#0a0a0c]">
                          <div className="flex justify-between items-center mb-4">
                            <h3 className="text-[10px] font-bold text-white/20 uppercase tracking-[0.2em] flex items-center gap-3">
                              <div className="w-[1px] h-3 bg-[#98c379]" />
                              Incoming_Response
                            </h3>
                            <CopyButton content={formatJson(log.response)} />
                          </div>
                          <ResizableContainer>
                            <JsonExplorer data={log.response} isWrapping={isWrapping} />
                          </ResizableContainer>
                        </div>
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            ))}
            <div className="pt-12 text-center opacity-10 uppercase tracking-[0.4em] font-bold text-[9px]">
              [ END_OF_LOG_STREAM ]
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

const HistoryEditor: React.FC = () => {
  const [history, setHistory] = useState<Message[]>([]);
  const [editingJson, setEditingJson] = useState<string>('');
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchHistory = async () => {
    try {
      const res = await fetch('/api/history');
      if (res.ok) {
        const data = await res.json();
        setHistory(data);
        setEditingJson(JSON.stringify(data, null, 2));
      }
    } catch (e) {
      setError("Failed to load history");
    }
  };

  useEffect(() => { fetchHistory(); }, []);

  const handleSave = async () => {
    setIsSaving(true);
    setError(null);
    try {
      const parsed = JSON.parse(editingJson);
      const res = await fetch('/api/history', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(parsed)
      });
      if (res.ok) {
        setHistory(parsed);
      } else {
        throw new Error(await res.text());
      }
    } catch (e: any) {
      setError(e.message);
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="flex items-center justify-between h-9 mb-6">
        <div className="flex items-center gap-2 text-white/60">
          <MessageSquare size={16} />
          <h3 className="text-[10px] font-bold uppercase tracking-[0.2em]">Dialogue_Buffer</h3>
        </div>
        <button
          onClick={handleSave}
          disabled={isSaving}
          className="flex items-center gap-2 px-3 py-1 bg-[#a3c2a3]/10 text-[#a3c2a3] hover:bg-[#a3c2a3]/20 rounded-sm border border-[#a3c2a3]/20 transition-all disabled:opacity-50"
        >
          <Save size={14} />
          <span className="text-[10px] font-bold uppercase tracking-wider">{isSaving ? 'Syncing...' : 'Sync_Buffer'}</span>
        </button>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-500/10 border border-red-500/20 rounded-sm flex items-center gap-3 text-red-400 text-[11px]">
          <AlertCircle size={14} />
          <span>{error}</span>
        </div>
      )}

      <div className="flex-1 min-h-0 relative">
        <CodeMirror
          value={editingJson}
          height="100%"
          theme={oneDark}
          extensions={[json(), jsonSchema(MESSAGE_SCHEMA), debugTheme]}
          onChange={(value) => setEditingJson(value)}
          className="h-full border border-white/10 rounded-sm overflow-hidden relative z-0"
          basicSetup={{
            lineNumbers: true,
            foldGutter: true,
            dropCursor: true,
            allowMultipleSelections: false,
            indentOnInput: true,
          }}
          style={{ fontSize: '11px' }}
        />
      </div>
    </div>
  );
};

const WorldEditor: React.FC = () => {
  const [world, setWorld] = useState<WorldState | null>(null);
  const [selectedEntityId, setSelectedEntityId] = useState<string | null>(null);
  const [editingJson, setEditingJson] = useState<string>('');
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchWorld = async () => {
    try {
      const res = await fetch('/api/world');
      if (res.ok) {
        setWorld(await res.json());
      }
    } catch (e) {
      setError("Failed to load world state");
    }
  };

  useEffect(() => { fetchWorld(); }, []);

  const selectEntity = (entity: WorldEntity) => {
    setSelectedEntityId(entity.id);
    setEditingJson(JSON.stringify(entity, null, 2));
    setError(null);
  };

  const handleSave = async () => {
    if (!selectedEntityId) return;
    setIsSaving(true);
    setError(null);
    try {
      const parsed = JSON.parse(editingJson);
      const res = await fetch('/api/world/entity', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(parsed)
      });
      if (res.ok) {
        await fetchWorld();
      } else {
        throw new Error(await res.text());
      }
    } catch (e: any) {
      setError(e.message);
    } finally {
      setIsSaving(false);
    }
  };

  const allEntities = world ? [
    ...Object.values(world.characters),
    ...Object.values(world.locations),
    ...Object.values(world.objects)
  ] : [];

  return (
    <div className="flex h-full overflow-hidden gap-6">
      <div className="w-1/3 flex flex-col border-r border-white/10 pr-6">
        <div className="flex items-center gap-2 h-9 mb-6 text-white/60">
          <Database size={16} />
          <h3 className="text-[10px] font-bold uppercase tracking-[0.2em]">Entity_Manifest</h3>
        </div>
        <div className="flex-1 overflow-y-auto debug-scrollbar space-y-2">
          {allEntities.map(entity => (
            <button
              key={entity.id}
              onClick={() => selectEntity(entity)}
              className={`w-full text-left p-3 rounded-sm text-[11px] transition-all border ${selectedEntityId === entity.id
                ? 'bg-white/10 border-white/30 text-white'
                : 'bg-white/2 border-transparent text-white/40 hover:bg-white/5 hover:text-white/60'
                }`}
            >
              <div className="font-bold flex items-center justify-between">
                <span className="truncate mr-2">{entity.displayName}</span>
                <span className="text-[8px] opacity-40 px-1 border border-current rounded-sm uppercase flex-shrink-0">{entity.type}</span>
              </div>
              <div className="opacity-30 truncate text-[9px] mt-1 font-mono">{entity.id}</div>
            </button>
          ))}
          <button
            onClick={() => {
              const newId = `new_entity_${Date.now()}`;
              selectEntity({
                id: newId,
                type: 'OBJECT',
                displayName: 'New Entity',
                shortDescription: '',
                longDescription: '',
                attributes: {}
              } as WorldEntity);
            }}
            className="w-full text-left p-3 rounded-sm text-[10px] border border-dashed border-white/10 text-white/20 hover:border-white/30 hover:text-white/40 flex items-center gap-2 transition-all uppercase tracking-widest font-bold"
          >
            <Plus size={12} />
            <span>Append_Entity</span>
          </button>
        </div>
      </div>

      <div className="flex-1 flex flex-col min-w-0">
        {selectedEntityId ? (
          <div className="flex flex-col h-full">
            <div className="flex items-center justify-between h-9 mb-6">
              <h3 className="text-[10px] font-bold uppercase tracking-[0.2em] text-white/30 px-3 py-1 border border-white/5 bg-white/2 rounded-sm truncate max-w-[60%]">
                SUBJECT: {selectedEntityId}
              </h3>
              <button
                onClick={handleSave}
                disabled={isSaving}
                className="flex items-center gap-2 px-3 py-1 bg-white/5 text-white/60 hover:bg-white/10 hover:text-white rounded-sm border border-white/10 transition-all disabled:opacity-50"
              >
                <Save size={14} />
                <span className="text-[10px] font-bold uppercase tracking-wider">{isSaving ? 'Syncing...' : 'Update_Manifest'}</span>
              </button>
            </div>

            {error && (
              <div className="mb-4 p-3 bg-red-500/10 border border-red-500/20 rounded-sm flex items-center gap-3 text-red-400 text-[11px]">
                <AlertCircle size={14} />
                <span>{error}</span>
              </div>
            )}

            <div className="flex-1 min-h-0">
              <CodeMirror
                value={editingJson}
                height="100%"
                theme={oneDark}
                extensions={[json(), jsonSchema(ENTITY_SCHEMA), debugTheme]}
                onChange={(value) => setEditingJson(value)}
                className="h-full border border-white/10 rounded-sm overflow-hidden relative z-0"
                basicSetup={{
                  lineNumbers: true,
                  foldGutter: true,
                  dropCursor: true,
                  allowMultipleSelections: false,
                  indentOnInput: true,
                }}
                style={{ fontSize: '11px' }}
              />
            </div>
          </div>
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center text-white/5">
            <Database size={64} className="mb-6 opacity-50" />
            <p className="uppercase tracking-[0.4em] text-[10px] font-bold">Select_Manifest_Entry</p>
          </div>
        )}
      </div>
    </div>
  );
};

const ConsoleViewer: React.FC = () => {
  const [logs, setLogs] = useState<ConsoleLog[]>(consoleLogger.getLogs());
  const [filterKeyword, setFilterKeyword] = useState('');
  const [filterLevels, setFilterLevels] = useState<LogLevel[]>(['log', 'info', 'warn', 'error']);
  const [dateStart, setDateStart] = useState<string>('');
  const [dateEnd, setDateEnd] = useState<string>('');
  const [showFilters, setShowFilters] = useState(false);
  const [isWrapping, setIsWrapping] = useState(true);

  const fetchPersistedLogs = async () => {
    try {
      const response = await fetch('/api/debug/console');
      if (response.ok) {
        const persistedLogs = await response.json();
        // Backend returns args as string, we need to parse it for the UI
        const formattedLogs = persistedLogs.map((log: any) => ({
          ...log,
          timestamp: new Date(log.timestamp).getTime(),
          args: JSON.parse(log.args)
        }));
        consoleLogger.setLogs(formattedLogs);
      }
    } catch (error) {
      console.error("Failed to fetch persisted console logs:", error);
    }
  };

  useEffect(() => {
    fetchPersistedLogs();
    const unsubscribe = consoleLogger.subscribe(() => {
      setLogs(consoleLogger.getLogs());
    });
    return unsubscribe;
  }, []);

  const clearLogs = () => {
    consoleLogger.clearLogs();
    setLogs([]);
  };

  const toggleLevel = (level: LogLevel) => {
    setFilterLevels(prev => 
      prev.includes(level) 
        ? prev.filter(l => l !== level) 
        : [...prev, level]
    );
  };

  const filteredLogs = logs.filter(log => {
    if (!filterLevels.includes(log.level)) return false;

    if (filterKeyword) {
      try {
        const regex = new RegExp(filterKeyword, 'i');
        // Test message or any of the stringified args
        const contentToTest = log.message;
        if (!regex.test(contentToTest)) return false;
      } catch (e) {
        if (!log.message.toLowerCase().includes(filterKeyword.toLowerCase())) return false;
      }
    }

    if (dateStart) {
      const start = new Date(dateStart).getTime();
      if (log.timestamp < start) return false;
    }
    if (dateEnd) {
      const end = new Date(dateEnd).getTime();
      if (log.timestamp > end) return false;
    }

    return true;
  });

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="flex items-center justify-between h-9 mb-6 flex-shrink-0">
        <div className="flex items-center gap-2 text-white/60">
          <Monitor size={16} />
          <h3 className="text-[10px] font-bold uppercase tracking-[0.2em]">CONSOLE_LOGS</h3>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setIsWrapping(!isWrapping)}
            className={`flex items-center gap-2 px-3 py-1 rounded-sm border transition-all ${
              isWrapping
                ? 'bg-white/10 text-white border-white/20'
                : 'bg-white/5 text-white/40 border-white/10 hover:bg-white/10 hover:text-white/60'
            }`}
            title={isWrapping ? "Disable text wrap" : "Enable text wrap"}
          >
            <WrapText size={14} className={isWrapping ? 'text-blue-400' : ''} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Wrap</span>
          </button>
          <button
            onClick={() => setShowFilters(!showFilters)}
            className={`flex items-center gap-2 px-3 py-1 rounded-sm border transition-all ${
              showFilters
                ? 'bg-white/10 text-white border-white/20'
                : 'bg-white/5 text-white/40 border-white/10 hover:bg-white/10 hover:text-white/60'
            }`}
          >
            <Filter size={14} className={showFilters ? 'text-[#ff6b35]' : ''} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Filter</span>
          </button>
          <button
            onClick={fetchPersistedLogs}
            className="flex items-center gap-2 px-3 py-1 bg-white/5 text-white/40 hover:bg-white/10 hover:text-white rounded-sm border border-white/10 transition-all"
          >
            <RefreshCw size={14} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Sync</span>
          </button>
          <button
            onClick={clearLogs}
            className="flex items-center gap-2 px-3 py-1 bg-white/5 text-white/40 hover:bg-red-500/20 hover:text-red-400 rounded-sm border border-white/5 hover:border-red-500/20 transition-all"
          >
            <Trash2 size={14} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Clear</span>
          </button>
        </div>
      </div>

      <AnimatePresence>
        {showFilters && (
          <motion.div
            initial={{ height: 0, opacity: 0, marginBottom: 0 }}
            animate={{ height: 'auto', opacity: 1, marginBottom: 24 }}
            exit={{ height: 0, opacity: 0, marginBottom: 0 }}
            className="flex flex-col gap-4 p-4 bg-white/[0.02] border border-white/5 rounded-sm overflow-hidden"
          >
            <div className="flex flex-wrap gap-4 items-end">
              <div className="flex-1 min-w-[200px]">
                <label className="block text-[9px] font-bold text-white/30 uppercase tracking-widest mb-1.5 flex items-center gap-2">
                  <Search size={10} />
                  Keyword / Regex
                </label>
                <input
                  type="text"
                  value={filterKeyword}
                  onChange={(e) => setFilterKeyword(e.target.value)}
                  placeholder="Filter logs..."
                  className="w-full bg-[#0d0d0d] border border-white/10 rounded-sm px-3 py-1.5 text-[11px] font-mono text-white/80 focus:outline-none focus:border-[#ff6b35]/40 transition-colors placeholder:text-white/10"
                />
              </div>

              <div className="flex gap-2">
                {(['log', 'info', 'warn', 'error'] as LogLevel[]).map(level => (
                  <button
                    key={level}
                    onClick={() => toggleLevel(level)}
                    className={`px-2.5 py-1.5 rounded-sm text-[9px] font-bold uppercase border transition-all ${
                      filterLevels.includes(level)
                        ? level === 'error' ? 'bg-red-500/10 border-red-500/30 text-red-400' :
                          level === 'warn' ? 'bg-yellow-500/10 border-yellow-500/30 text-yellow-400' :
                          level === 'info' ? 'bg-blue-500/10 border-blue-500/30 text-blue-400' :
                          'bg-white/10 border-white/30 text-white'
                        : 'bg-white/2 border-white/5 text-white/20 hover:text-white/40'
                    }`}
                  >
                    {level}
                  </button>
                ))}
              </div>
            </div>

            <div className="flex flex-wrap gap-4">
              <div className="flex-1 min-w-[150px]">
                <label className="block text-[9px] font-bold text-white/30 uppercase tracking-widest mb-1.5 flex items-center gap-2">
                  <Calendar size={10} />
                  Date Start
                </label>
                <input
                  type="datetime-local"
                  value={dateStart}
                  onChange={(e) => setDateStart(e.target.value)}
                  className="w-full bg-[#0d0d0d] border border-white/10 rounded-sm px-3 py-1.5 text-[11px] font-mono text-white/80 focus:outline-none focus:border-[#ff6b35]/40 transition-colors [color-scheme:dark]"
                />
              </div>
              <div className="flex-1 min-w-[150px]">
                <label className="block text-[9px] font-bold text-white/30 uppercase tracking-widest mb-1.5 flex items-center gap-2">
                  <Clock size={10} />
                  Date End
                </label>
                <input
                  type="datetime-local"
                  value={dateEnd}
                  onChange={(e) => setDateEnd(e.target.value)}
                  className="w-full bg-[#0d0d0d] border border-white/10 rounded-sm px-3 py-1.5 text-[11px] font-mono text-white/80 focus:outline-none focus:border-[#ff6b35]/40 transition-colors [color-scheme:dark]"
                />
              </div>
              <div className="flex items-end pb-0.5">
                <button
                  onClick={() => {
                    setFilterKeyword('');
                    setFilterLevels(['log', 'info', 'warn', 'error']);
                    setDateStart('');
                    setDateEnd('');
                  }}
                  className="px-3 py-1.5 text-[9px] font-bold uppercase text-white/30 hover:text-white transition-colors"
                >
                  Reset_Filters
                </button>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <div className="flex-1 overflow-y-auto debug-scrollbar font-mono text-[11px] space-y-1 pr-1">
        {filteredLogs.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-white/10 py-20 grayscale opacity-50">
            <Monitor size={48} className="mb-4" />
            <p className="uppercase tracking-[0.3em] text-[10px] font-bold">{logs.length > 0 ? 'No_Matches_Found' : 'No_Logs_Recorded'}</p>
          </div>
        ) : (
          <div className={!isWrapping ? "w-fit min-w-full" : ""}>
            {[...filteredLogs].reverse().map((log) => (
              <div key={log.id} className="flex gap-3 py-1 px-2 border-b border-white/[0.03] hover:bg-white/[0.02] group">
                <span className="text-white/20 select-none w-24 flex-shrink-0 text-[10px] whitespace-nowrap">
                  {new Date(log.timestamp).toLocaleString([], { 
                    month: '2-digit', 
                    day: '2-digit', 
                    hour: '2-digit', 
                    minute: '2-digit', 
                    second: '2-digit',
                    hour12: false 
                  }).replace(',', '')}
                </span>
                <span className={`uppercase font-bold text-[9px] w-12 flex-shrink-0 mt-0.5 ${
                  log.level === 'error' ? 'text-red-400' :
                  log.level === 'warn' ? 'text-yellow-400' :
                  log.level === 'info' ? 'text-blue-400' :
                  'text-white/40'
                }`}>
                  [{log.level}]
                </span>
                <div className="flex-1 flex flex-wrap items-start gap-x-2">
                  {log.args.map((arg, i) => {
                    if (typeof arg === 'string') {
                      return (
                        <span key={i} className={`${isWrapping ? 'whitespace-pre-wrap break-all' : 'whitespace-pre'} ${
                          log.level === 'error' ? 'text-red-300/90' :
                          log.level === 'warn' ? 'text-yellow-200/80' :
                          'text-gray-300'
                        }`}>
                          {arg}
                        </span>
                      );
                    }
                    if (arg === null || arg === undefined || typeof arg !== 'object') {
                      return (
                        <span key={i} className="text-white/40 tabular-nums">
                          {String(arg)}
                        </span>
                      );
                    }
                    return (
                      <div key={i} className="w-full mt-1 mb-2 p-2 bg-white/[0.02] border border-white/10 rounded-sm overflow-x-auto debug-scrollbar">
                        <JsonNode value={arg} depth={1} isWrapping={isWrapping} />
                      </div>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>
        )}
        {logs.length > 0 && (
          <div className="pt-8 text-center opacity-10 uppercase tracking-[0.4em] font-bold text-[9px]">
            [ START_OF_LOG_STREAM ]
          </div>
        )}
      </div>
    </div>
  );
};

export const DebugPanel: React.FC = () => {
  const [isOpen, setIsOpen] = useState(false);
  const [activeTab, setActiveTab] = useState<'logs' | 'console' | 'history' | 'world'>('logs');

  const TabButton: React.FC<{
    id: 'logs' | 'console' | 'history' | 'world',
    label: string,
    icon: React.ReactNode
  }> = ({ id, label, icon }) => (
    <button
      onClick={() => setActiveTab(id)}
      className={`px-4 py-3 flex items-center gap-2 border-b transition-all ${activeTab === id
        ? 'border-white text-white bg-white/5'
        : 'border-transparent text-white/30 hover:text-white/60 hover:bg-white/2'
        }`}
    >
      {icon}
      <span className="text-[10px] font-bold uppercase tracking-[0.2em]">{label}</span>
    </button>
  );

  return (
    <>
      <button
        id="debug-panel-toggle"
        onClick={() => setIsOpen(true)}
        className="fixed bottom-8 right-8 z-50 h-11 min-w-[2.75rem] px-3 bg-[#1a1a1a] border border-[#ff6b35]/30 rounded-full text-[#ff6b35] hover:bg-[#ff6b35] hover:text-white transition-all duration-300 shadow-lg group flex items-center justify-center overflow-hidden"
        title="Open Debug Panel"
      >
        <div className="flex items-center justify-center">
          <Bug size={20} className="shrink-0" />
          <span className="max-w-0 overflow-hidden group-hover:max-w-[120px] group-hover:ml-3 transition-all duration-300 ease-in-out whitespace-nowrap text-[12px] uppercase tracking-widest font-sans font-bold">
            Debug
          </span>
        </div>
      </button>

      <AnimatePresence>
        {isOpen && (
          <>
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              onClick={() => setIsOpen(false)}
              className="fixed inset-0 bg-black/40 backdrop-blur-[1px] z-40"
            />
            <motion.div
              id="debug-panel-modal"
              initial={{ opacity: 0, x: 300 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 300 }}
              className="fixed inset-y-0 right-0 w-full md:w-160 bg-[#0a0a0a] text-gray-100 shadow-2xl z-50 flex flex-col border-l border-white/10 font-mono text-sm"
            >
            <div className="flex items-center justify-between px-4 border-b border-white/10 bg-[#0a0a0a]/90 backdrop-blur-md sticky top-0 z-10">
              <div className="flex items-center">
                <TabButton id="logs" label="Logs" icon={<Terminal size={14} />} />
                <TabButton id="console" label="Console" icon={<Monitor size={14} />} />
                <TabButton id="history" label="History" icon={<MessageSquare size={14} />} />
                <TabButton id="world" label="World" icon={<Database size={14} />} />
              </div>
              <div className="flex items-center gap-3 pr-4">
                <button
                  onClick={() => setIsOpen(false)}
                  className="p-1.5 hover:bg-white/5 rounded-sm text-white/40 hover:text-white transition-colors"
                >
                  <X size={18} />
                </button>
              </div>
            </div>

            <div className="flex-1 p-6 min-h-0 flex flex-col">
              {activeTab === 'logs' && <LlmTraceViewer />}
              {activeTab === 'console' && <ConsoleViewer />}
              {activeTab === 'history' && <HistoryEditor />}
              {activeTab === 'world' && <WorldEditor />}
            </div>
          </motion.div>
          </>
        )}
      </AnimatePresence>
    </>
  );
};

