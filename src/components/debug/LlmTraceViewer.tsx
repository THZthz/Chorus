/**
 * Elysian Dialogue — cinematic RPG-style dialogue engine
 * Copyright (C) 2026  Amias
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

import React, { useEffect, useRef, useState } from "react";
import { Terminal, ChevronDown, RefreshCw, Trash2, Bug, WrapText, Clock } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { LlmLog } from "@/server/models/debug";
import { TOOL_NAMES } from "@/shared/constants";
import { CopyButton } from "@/components/debug/CopyButton";
import { JsonExplorer } from "@/components/debug/JsonExplorer";
import { JsonNode } from "@/components/debug/JsonNode";

const MAX_CONTENT_PREVIEW = 500;

function normalizeDbStep(s: any) {
  const toolCalls = s.tool_calls ? JSON.parse(s.tool_calls) : [];
  const toolResults = s.tool_results ? JSON.parse(s.tool_results) : [];
  const usage = s.usage ? JSON.parse(s.usage) : null;
  return {
    stepNumber: s.step_number,
    finishReason: s.finish_reason,
    usage,
    text: s.text,
    duration_ms: s.duration_ms,
    userPrompt: s.user_prompt,
    reasoning: s.reasoning,
    toolCalls: toolCalls.map((tc: any) => {
      const result = toolResults.find((r: any) => r.toolCallId === tc.toolCallId);
      return { ...tc, output: result?.output };
    }),
  };
}

function normalizeLegacyStep(s: any) {
  const contents = s.content || [];
  const toolCalls = contents.filter((c: any) => c.type === "tool-call");
  const toolResults = contents.filter((c: any) => c.type === "tool-result");
  return {
    stepNumber: s.stepNumber,
    finishReason: s.finishReason,
    usage: s.usage,
    text: s.text,
    duration_ms: s.duration_ms,
    userPrompt: s.user_prompt,
    reasoning: s.reasoning,
    toolCalls: toolCalls.map((tc: any) => {
      const result = toolResults.find((r: any) => r.toolCallId === tc.toolCallId);
      return { ...tc, output: result?.output ?? result?.result };
    }),
  };
}

function renderUserPrompt(rawPrompt: string) {
  try {
    const msgs = JSON.parse(rawPrompt);
    if (!Array.isArray(msgs) || msgs.length === 0) return null;
    const lastMsg = msgs[msgs.length - 1];
    const content =
      typeof lastMsg.content === "string" ? lastMsg.content : JSON.stringify(lastMsg.content);
    return (
      <>
        <div className="text-[10px] text-white/40 font-mono mb-1">
          {msgs.length} message{msgs.length !== 1 ? "s" : ""}
          {lastMsg.role && <span className="text-white/20"> · last: {lastMsg.role}</span>}
        </div>
        <div className="text-[11px] text-white/60 whitespace-pre-wrap break-words leading-relaxed pl-2 border-l-2 border-white/[0.06] max-h-[120px] overflow-auto debug-scrollbar">
          {content.length > MAX_CONTENT_PREVIEW
            ? content.slice(0, MAX_CONTENT_PREVIEW) + "..."
            : content}
        </div>
      </>
    );
  } catch {
    return (
      <div className="text-[11px] text-white/50 whitespace-pre-wrap break-words leading-relaxed">
        {rawPrompt}
      </div>
    );
  }
}

export const LlmTraceViewer: React.FC = () => {
  const [logs, setLogs] = useState<LlmLog[]>([]);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isWrapping, setIsWrapping] = useState(true);

  const fetchLogs = async () => {
    setIsLoading(true);
    try {
      const response = await fetch("/api/debug/logs");
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
      await fetch("/api/debug/logs/clear", { method: "POST" });
      setLogs([]);
    } catch (error) {
      console.error("Failed to clear logs:", error);
    }
  };

  const [autoRefresh, setAutoRefresh] = useState(false);
  const [responseHeight, setResponseHeight] = useState(300);
  const dragRef = useRef<{
    startY: number;
    startHeight: number;
  } | null>(null);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!dragRef.current) return;
      const delta = e.clientY - dragRef.current.startY;
      const newHeight = Math.max(80, dragRef.current.startHeight + delta);
      setResponseHeight(newHeight);
    };
    const onUp = () => {
      dragRef.current = null;
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  useEffect(() => {
    fetchLogs();
  }, []);

  useEffect(() => {
    if (!autoRefresh) return;
    const interval = setInterval(fetchLogs, 2000);
    return () => clearInterval(interval);
  }, [autoRefresh]);

  const readableToolName = (name: string) => name.replace(/([a-z])([A-Z])/g, "$1 $2");

  const isToolError = (output: unknown): boolean => {
    if (typeof output !== "string") return false;
    return /^(ERROR|VALIDATION FAILED)/.test(output);
  };

  const formatJson = (jsonStr: string | null) => {
    if (!jsonStr) return "N/A";
    try {
      return JSON.stringify(JSON.parse(jsonStr), null, 2);
    } catch (e) {
      return jsonStr;
    }
  };

  const extractDynamicSections = (
    systemPrompt: string,
  ): { entities: string | null; plots: string | null } => {
    // Extract from section header to the next structural delimiter (--- or ## or end)
    const entitiesMatch = systemPrompt.match(/## WORLD ENTITIES\n\n([\s\S]*?)\n\n---/);
    const entities = entitiesMatch ? entitiesMatch[1].trim() : null;
    const plotsMatch = systemPrompt.match(/## ACTIVE PLOTS\n\n([\s\S]*?)(?:\n\n---|\n\n## |$)/);
    const plots = plotsMatch ? plotsMatch[1].trim() : null;
    return { entities, plots };
  };

  const renderToolOutput = (output: unknown, compact?: boolean) => {
    if (output === null || output === undefined) return null;
    if (typeof output !== "string") {
      return (
        <JsonExplorer
          data={JSON.stringify(output)}
          isWrapping={isWrapping}
          className={`overflow-auto ${compact ? "max-h-[100px]" : "max-h-[150px]"}`}
        />
      );
    }
    // Try to parse string as JSON for structured display
    try {
      const parsed = JSON.parse(output);
      if (parsed !== null && typeof parsed === "object") {
        return (
          <div
            className={`debug-scrollbar bg-transparent overflow-auto ${compact ? "max-h-[100px]" : "max-h-[150px]"}`}
          >
            <JsonNode value={parsed} depth={0} isWrapping={isWrapping} />
          </div>
        );
      }
    } catch {}
    // Fallback to plain text
    return (
      <div
        className={`whitespace-pre-wrap break-words leading-relaxed ${
          compact ? "text-[10px]" : "text-[11px]"
        } ${output.includes("SUCCESS") ? "text-[#98c379]" : "text-white/40"}`}
      >
        {output}
      </div>
    );
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
                ? "bg-white/10 text-white border-white/20"
                : "bg-white/5 text-white/40 border-white/10 hover:bg-white/10 hover:text-white/60"
            }`}
            title={isWrapping ? "Disable text wrap" : "Enable text wrap"}
          >
            <WrapText size={14} className={isWrapping ? "text-blue-400" : ""} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Wrap</span>
          </button>
          <button
            onClick={fetchLogs}
            disabled={isLoading}
            className="flex items-center gap-2 px-3 py-1 bg-white/5 text-white/60 hover:bg-white/10 hover:text-white rounded-sm border border-white/10 transition-all disabled:opacity-50"
          >
            <RefreshCw size={14} className={isLoading ? "animate-spin" : ""} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Refresh</span>
          </button>
          <button
            onClick={clearLogs}
            className="flex items-center gap-2 px-3 py-1 bg-white/5 text-white/40 hover:bg-red-500/20 hover:text-red-400 rounded-sm border border-white/5 hover:border-red-500/20 transition-all"
          >
            <Trash2 size={14} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Clear</span>
          </button>
          <button
            onClick={() => setAutoRefresh(!autoRefresh)}
            className={`flex items-center gap-2 px-3 py-1 rounded-sm border transition-all ${
              autoRefresh
                ? "bg-green-500/10 text-green-400 border-green-500/20"
                : "bg-white/5 text-white/40 border-white/10 hover:bg-white/10 hover:text-white/60"
            }`}
            title={autoRefresh ? "Disable auto-refresh" : "Auto-refresh every 2s"}
          >
            <Clock size={14} className={autoRefresh ? "text-green-400 animate-pulse" : ""} />
            <span className="text-[10px] font-bold uppercase tracking-wider">Live</span>
          </button>
        </div>
      </div>
      <div className="flex-1 space-y-4 overflow-y-auto debug-scrollbar pr-1">
        {logs.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-white/10 py-20 grayscale opacity-50">
            <Bug size={48} className="mb-4" />
            <p className="uppercase tracking-[0.3em] text-[10px] font-bold">
              Awaiting_Transmission...
            </p>
          </div>
        ) : (
          <div className="space-y-4">
            {logs.map((log) => (
              <div
                key={log.id}
                className={`border rounded-sm overflow-hidden transition-all duration-300 ${
                  expandedId === log.id
                    ? "border-white/[0.15] bg-white/[0.03]"
                    : "border-white/5 hover:border-white/[0.08] bg-white/[0.015]"
                }`}
              >
                <button
                  onClick={() => setExpandedId(expandedId === log.id ? null : log.id)}
                  className="w-full text-left p-4 flex items-center justify-between gap-4 group"
                >
                  <div className="flex items-center gap-4 min-w-0">
                    <div
                      className={`transition-transform duration-300 ${expandedId === log.id ? "rotate-0" : "-rotate-90 opacity-40"}`}
                    >
                      <ChevronDown size={14} />
                    </div>
                    {log.label && (
                      <span
                        className={`px-2 py-0.5 rounded-sm text-[9px] font-bold tracking-widest uppercase border ${
                          log.label === "Assistant"
                            ? "bg-purple-500/5 text-purple-400 border-purple-500/20"
                            : "bg-white/5 text-white/60 border-white/10"
                        }`}
                      >
                        {log.label}
                      </span>
                    )}
                    <span
                      className={`px-2 py-0.5 rounded-sm text-[9px] font-bold tracking-widest uppercase border ${
                        log.status === "ERROR"
                          ? "bg-red-500/5 text-red-400 border-red-500/20"
                          : "bg-white/5 text-white/60 border-white/10"
                      }`}
                    >
                      {log.status}
                    </span>
                    <span className="text-white/30 font-mono text-[10px] tabular-nums tracking-widest whitespace-nowrap">
                      {new Date(log.timestamp.replace(" ", "T") + "Z")
                        .toLocaleString([], {
                          year: "2-digit",
                          month: "2-digit",
                          day: "2-digit",
                          hour: "2-digit",
                          minute: "2-digit",
                          second: "2-digit",
                          hour12: false,
                        })
                        .replace(",", "")}
                    </span>
                  </div>
                  <div className="flex items-center gap-3 font-mono text-[10px] opacity-40 group-hover:opacity-100 transition-opacity">
                    {log.steps && log.steps.length > 0 && (
                      <span className="tracking-widest">
                        {log.steps.length} step{log.steps.length !== 1 ? "s" : ""}
                      </span>
                    )}
                    <span className="tracking-widest">{log.duration}ms</span>
                  </div>
                </button>

                <AnimatePresence>
                  {expandedId === log.id && (
                    <motion.div
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: "auto", opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{ duration: 0.3 }}
                      className="overflow-hidden border-t border-white/10"
                    >
                      <div className="divide-y divide-white/[0.03]">
                        {(() => {
                          // Parse request for display
                          let req: any = {};
                          try {
                            req = JSON.parse(log.request);
                          } catch (e) {}

                          // Parse response for metadata
                          let resp: any = {};
                          try {
                            resp =
                              typeof log.response === "string"
                                ? JSON.parse(log.response || "{}")
                                : (log.response ?? {});
                          } catch (e) {}

                          // Build normalized steps with tool calls paired to their results
                          let steps: any[] = [];
                          if (log.steps && log.steps.length > 0) {
                            steps = log.steps.map(normalizeDbStep);
                          } else {
                            // Fallback for old logs without structured steps
                            try {
                              const parsed =
                                typeof log.response === "string"
                                  ? JSON.parse(log.response || "{}")
                                  : log.response;
                              const rawSteps = parsed?.steps || [];
                              steps = rawSteps.map(normalizeLegacyStep);
                            } catch (e) {}
                          }

                          if (steps.length === 0) return null;

                          const totalTokens = steps.reduce((sum: number, s: any) => {
                            return sum + (s.usage?.totalTokens || 0);
                          }, 0);

                          return (
                            <div className="p-5 bg-[#0f1013]">
                              <h3 className="text-[10px] font-bold text-white/20 uppercase tracking-[0.2em] flex items-center gap-3 mb-6">
                                <div className="w-[1px] h-3 bg-[#eab308]" />
                                PARSED_EXCHANGE
                              </h3>

                              {/* Request summary */}
                              <div className="mb-6 p-4 bg-white/[0.02] border border-white/5 rounded-sm">
                                <div className="text-[9px] font-bold text-white/20 uppercase tracking-widest mb-3">
                                  Player Input
                                </div>
                                <div className="text-[12px] text-white/80 font-medium leading-relaxed mb-4 pl-3 border-l-2 border-[#61afef]/40 whitespace-pre-wrap break-words">
                                  {req.userInput || "(no input)"}
                                </div>
                                <div className="flex flex-wrap gap-3 text-[10px] font-mono text-white/40">
                                  <span className="px-2 py-0.5 bg-white/[0.03] border border-white/5 rounded-sm">
                                    model:{" "}
                                    <span className="text-[#d19a66]">{req.model || "unknown"}</span>
                                  </span>
                                  <span className="px-2 py-0.5 bg-white/[0.03] border border-white/5 rounded-sm">
                                    history:{" "}
                                    <span className="text-[#d19a66]">
                                      {(req.history || []).length} msgs
                                    </span>
                                  </span>
                                  {req.tools && (
                                    <span className="px-2 py-0.5 bg-white/[0.03] border border-white/5 rounded-sm">
                                      tools:{" "}
                                      <span className="text-[#98c379]">{req.tools.join(", ")}</span>
                                    </span>
                                  )}
                                </div>
                                {/* Response metadata */}
                                {(resp.finishReason || resp.totalUsage) && (
                                  <div className="mt-3 pt-3 border-t border-white/5 flex flex-wrap gap-3 text-[10px] font-mono items-center">
                                    {resp.finishReason && (
                                      <span
                                        className={`px-1.5 py-0.5 rounded-sm text-[8px] font-bold uppercase tracking-widest border ${
                                          resp.finishReason === "stop"
                                            ? "bg-[#98c379]/5 text-[#98c379] border-[#98c379]/20"
                                            : resp.finishReason === "tool-calls"
                                              ? "bg-[#61afef]/5 text-[#61afef] border-[#61afef]/20"
                                              : "bg-white/5 text-white/40 border-white/10"
                                        }`}
                                      >
                                        {resp.finishReason}
                                      </span>
                                    )}
                                    {resp.totalUsage && (
                                      <>
                                        <span className="text-white/40">
                                          in{" "}
                                          <span className="text-[#d19a66] tabular-nums">
                                            {resp.totalUsage.inputTokens ?? 0}
                                          </span>
                                        </span>
                                        <span className="text-white/40">
                                          out{" "}
                                          <span className="text-[#d19a66] tabular-nums">
                                            {resp.totalUsage.outputTokens ?? 0}
                                          </span>
                                        </span>
                                        <span className="text-white/40">
                                          tot{" "}
                                          <span className="text-[#d19a66] tabular-nums">
                                            {resp.totalUsage.totalTokens ?? 0}
                                          </span>
                                        </span>
                                      </>
                                    )}
                                  </div>
                                )}
                              </div>

                              {/* System Context — dynamic portions of the system prompt */}
                              {req.system &&
                                (() => {
                                  const { entities, plots } = extractDynamicSections(
                                    String(req.system),
                                  );
                                  if (!entities && !plots) return null;
                                  return (
                                    <div className="mb-6 p-4 bg-[#0a0a0c] border border-white/5 rounded-sm">
                                      <div className="text-[9px] font-bold text-white/20 uppercase tracking-widest mb-3">
                                        System Context
                                        <span className="text-white/10 ml-1">
                                          (dynamic content injected into prompt)
                                        </span>
                                      </div>
                                      {entities && (
                                        <div className="mb-3">
                                          <div className="flex items-center gap-2 mb-1">
                                            <div className="w-[1px] h-3 bg-[#e06c75]" />
                                            <span className="text-[9px] font-bold text-white/25 uppercase tracking-wider">
                                              World Entities
                                            </span>
                                          </div>
                                          <pre className="text-[10px] text-white/50 font-mono whitespace-pre-wrap break-words leading-relaxed bg-white/[0.01] p-3 rounded-sm border border-white/5 max-h-[200px] overflow-auto debug-scrollbar">
                                            {entities}
                                          </pre>
                                        </div>
                                      )}
                                      {plots && (
                                        <div>
                                          <div className="flex items-center gap-2 mb-1">
                                            <div className="w-[1px] h-3 bg-[#eab308]" />
                                            <span className="text-[9px] font-bold text-white/25 uppercase tracking-wider">
                                              Active Plots
                                            </span>
                                          </div>
                                          <pre className="text-[10px] text-white/50 font-mono whitespace-pre-wrap break-words leading-relaxed bg-white/[0.01] p-3 rounded-sm border border-white/5 max-h-[200px] overflow-auto debug-scrollbar">
                                            {plots}
                                          </pre>
                                        </div>
                                      )}
                                    </div>
                                  );
                                })()}

                              {/* Step timeline */}
                              <div className="space-y-4">
                                {steps.map((step: any, i: number) => (
                                  <div
                                    key={i}
                                    className="relative pl-4 border-l-2 border-white/[0.06]"
                                  >
                                    {/* Step header */}
                                    <div className="flex items-center gap-3 mb-2 flex-wrap">
                                      <span className="text-[10px] font-bold text-white/40 font-mono tracking-wider">
                                        Step {step.stepNumber ?? i + 1}
                                      </span>
                                      <span
                                        className={`px-1.5 py-0.5 rounded-sm text-[8px] font-bold uppercase tracking-widest border ${
                                          step.finishReason === "tool-calls"
                                            ? "bg-[#61afef]/5 text-[#61afef] border-[#61afef]/20"
                                            : step.finishReason === "stop"
                                              ? "bg-[#98c379]/5 text-[#98c379] border-[#98c379]/20"
                                              : "bg-white/5 text-white/40 border-white/10"
                                        }`}
                                      >
                                        {step.finishReason || "unknown"}
                                      </span>
                                      {step.usage && (
                                        <span className="text-[10px] font-mono text-white/25 tabular-nums">
                                          {step.usage.inputTokens ?? 0} in ·{" "}
                                          {step.usage.outputTokens ?? 0} out
                                          {step.usage.totalTokens != null &&
                                            ` · ${step.usage.totalTokens} tot`}
                                        </span>
                                      )}
                                      {step.duration_ms != null && (
                                        <span className="text-[10px] font-mono text-white/20 tabular-nums ml-auto">
                                          {step.duration_ms}ms
                                        </span>
                                      )}
                                    </div>

                                    {/* User prompt */}
                                    {step.userPrompt && (
                                      <div className="mb-2 p-3 bg-[#1a1a2e]/40 border border-[#61afef]/10 rounded-sm">
                                        <div className="flex items-center gap-2 mb-1">
                                          <div className="w-[1px] h-3 bg-[#61afef]" />
                                          <span className="text-[9px] font-bold text-[#61afef]/60 uppercase tracking-wider">
                                            Prompt
                                          </span>
                                        </div>
                                        {renderUserPrompt(step.userPrompt)}
                                      </div>
                                    )}

                                    {/* Text content */}
                                    {step.text && (
                                      <div className="mb-2 p-3 bg-white/[0.01] border border-white/5 rounded-sm text-[11px] text-white/50 italic whitespace-pre-wrap break-words leading-relaxed">
                                        {step.text}
                                      </div>
                                    )}

                                    {/* Reasoning */}
                                    {step.reasoning && (
                                      <div className="mb-2 p-3 bg-[#1a1a2e]/30 border border-[#c678dd]/10 rounded-sm">
                                        <div className="flex items-center gap-2 mb-1">
                                          <div className="w-[1px] h-3 bg-[#c678dd]" />
                                          <span className="text-[9px] font-bold text-[#c678dd]/60 uppercase tracking-wider">
                                            Reasoning
                                          </span>
                                        </div>
                                        <div className="text-[11px] text-white/50 whitespace-pre-wrap break-words leading-relaxed italic max-h-[200px] overflow-auto debug-scrollbar">
                                          {step.reasoning}
                                        </div>
                                      </div>
                                    )}

                                    {/* Tool calls */}
                                    {step.toolCalls &&
                                      step.toolCalls.length > 0 &&
                                      step.toolCalls.map((call: any, ci: number) => {
                                        const input = call.input || call.args || {};
                                        const callIsError = isToolError(call.output);
                                        const isGenDialogue =
                                          call.toolName === TOOL_NAMES.GENERATE_DIALOGUE;
                                        const isWorldUpdate =
                                          call.toolName === TOOL_NAMES.UPDATE_ENTITY;
                                        const isPlotStatus =
                                          call.toolName === TOOL_NAMES.UPDATE_PLOT;
                                        const isCreatePlot =
                                          call.toolName === TOOL_NAMES.CREATE_PLOT;
                                        const isGetAllEntities =
                                          call.toolName === TOOL_NAMES.LIST_ENTITIES;
                                        const isQueryEntity =
                                          call.toolName === TOOL_NAMES.GET_ENTITY;
                                        const isGetPlot = call.toolName === TOOL_NAMES.GET_PLOT;

                                        return (
                                          <div
                                            key={ci}
                                            className={`mb-2 p-3 rounded-sm ${
                                              callIsError
                                                ? "bg-red-500/[0.04] border border-red-500/30"
                                                : "bg-white/[0.02] border border-white/5"
                                            }`}
                                          >
                                            <div className="flex items-center justify-between mb-2">
                                              <h4 className="text-[10px] font-bold text-white/40 uppercase tracking-[0.15em] flex items-center gap-2">
                                                <div
                                                  className={`w-[1px] h-3 ${
                                                    isGenDialogue
                                                      ? "bg-[#c678dd]"
                                                      : isWorldUpdate
                                                        ? "bg-[#61afef]"
                                                        : isPlotStatus
                                                          ? "bg-[#eab308]"
                                                          : isCreatePlot
                                                            ? "bg-[#98c379]"
                                                            : "bg-white/20"
                                                  }`}
                                                />
                                                {readableToolName(call.toolName)}
                                                {callIsError && (
                                                  <span className="px-1 py-[1px] rounded-sm text-[8px] font-bold uppercase tracking-widest bg-red-500/20 text-red-400 border border-red-500/30 leading-none">
                                                    Error
                                                  </span>
                                                )}
                                              </h4>
                                              <CopyButton
                                                content={JSON.stringify(input, null, 2)}
                                              />
                                            </div>

                                            <div className="text-[11px] text-white/50 font-mono">
                                              {isGenDialogue && (
                                                <div>
                                                  <div className="mb-2">
                                                    <span className="text-white/40">
                                                      {(input.messages || []).length} messages,{" "}
                                                    </span>
                                                    <span className="text-white/40">
                                                      {(input.options || []).length} options
                                                    </span>
                                                  </div>
                                                  <JsonExplorer
                                                    data={JSON.stringify(input)}
                                                    isWrapping={isWrapping}
                                                    className="max-h-[300px] overflow-auto"
                                                  />
                                                </div>
                                              )}
                                              {isWorldUpdate && (
                                                <div>
                                                  <div className="mb-2 text-white/40">
                                                    <span className="text-[#d19a66] font-mono">
                                                      {input.id || "?"}
                                                    </span>
                                                    {input.shortDescription != null && (
                                                      <span className="text-white/30">
                                                        {" "}
                                                        shortDescription
                                                      </span>
                                                    )}
                                                    {input.longDescription != null && (
                                                      <span className="text-white/30">
                                                        {" "}
                                                        longDescription
                                                      </span>
                                                    )}
                                                    {input.attributes != null && (
                                                      <span className="text-white/30">
                                                        {" "}
                                                        attributes
                                                      </span>
                                                    )}
                                                    {input.opinions != null && (
                                                      <span className="text-white/30">
                                                        {" "}
                                                        opinions
                                                      </span>
                                                    )}
                                                  </div>
                                                  <JsonExplorer
                                                    data={JSON.stringify(input)}
                                                    isWrapping={isWrapping}
                                                    className="max-h-[200px] overflow-auto"
                                                  />
                                                </div>
                                              )}
                                              {isPlotStatus && (
                                                <div>
                                                  <span className="text-[#d19a66]">{input.id}</span>
                                                  <span className="text-white/20"> → </span>
                                                  <span className="text-[#98c379]">
                                                    {input.status}
                                                  </span>
                                                </div>
                                              )}
                                              {isCreatePlot && (
                                                <div>
                                                  <div className="text-white/60">
                                                    "{input.title}"
                                                  </div>
                                                  {input.description && (
                                                    <div className="text-white/30 mt-1">
                                                      {input.description}
                                                    </div>
                                                  )}
                                                </div>
                                              )}
                                              {isGetAllEntities && (
                                                <div>
                                                  <span className="text-white/40">Fetch </span>
                                                  <span className="text-[#d19a66]">
                                                    {input.type ? input.type : "all types"}
                                                  </span>
                                                </div>
                                              )}
                                              {isQueryEntity && (
                                                <div>
                                                  {input.id ? (
                                                    <>
                                                      <span className="text-white/40">ID: </span>
                                                      <span className="text-[#d19a66] font-mono">
                                                        {input.id}
                                                      </span>
                                                    </>
                                                  ) : input.ids ? (
                                                    <>
                                                      <span className="text-white/40">
                                                        Bulk ({input.ids.length}):{" "}
                                                      </span>
                                                      <span className="text-[#d19a66] font-mono">
                                                        [{input.ids.slice(0, 3).join(", ")}
                                                        {input.ids.length > 3
                                                          ? `, +${input.ids.length - 3} more`
                                                          : ""}
                                                        ]
                                                      </span>
                                                    </>
                                                  ) : input.search ? (
                                                    <>
                                                      <span className="text-white/40">
                                                        Search:{" "}
                                                      </span>
                                                      <span className="text-[#98c379]">
                                                        "{input.search}"
                                                      </span>
                                                    </>
                                                  ) : (
                                                    <span className="text-[#e06c75]">
                                                      Missing query params
                                                    </span>
                                                  )}
                                                </div>
                                              )}
                                              {isGetPlot && (
                                                <div>
                                                  {input.id ? (
                                                    <>
                                                      <span className="text-white/40">Plot: </span>
                                                      <span className="text-[#d19a66] font-mono">
                                                        {input.id}
                                                      </span>
                                                    </>
                                                  ) : input.ids ? (
                                                    <>
                                                      <span className="text-white/40">
                                                        Bulk ({input.ids.length}):{" "}
                                                      </span>
                                                      <span className="text-[#d19a66] font-mono">
                                                        [{input.ids.slice(0, 3).join(", ")}
                                                        {input.ids.length > 3
                                                          ? `, +${input.ids.length - 3} more`
                                                          : ""}
                                                        ]
                                                      </span>
                                                    </>
                                                  ) : input.status ? (
                                                    <>
                                                      <span className="text-white/40">
                                                        Status filter:{" "}
                                                      </span>
                                                      <span className="text-[#eab308]">
                                                        {input.status}
                                                      </span>
                                                    </>
                                                  ) : (
                                                    <span className="text-white/40">All plots</span>
                                                  )}
                                                </div>
                                              )}
                                              {!isGenDialogue &&
                                                !isWorldUpdate &&
                                                !isPlotStatus &&
                                                !isCreatePlot &&
                                                !isGetAllEntities &&
                                                !isQueryEntity &&
                                                !isGetPlot && (
                                                  <JsonExplorer
                                                    data={JSON.stringify(input)}
                                                    isWrapping={isWrapping}
                                                    className="max-h-[200px] overflow-auto"
                                                  />
                                                )}
                                            </div>

                                            {call.output != null && (
                                              <div className="mt-2 pt-2 border-t border-white/5">
                                                {callIsError ? (
                                                  <div className="flex items-start gap-2">
                                                    <span className="mt-[1px] shrink-0 w-3.5 h-3.5 rounded-full bg-red-500/20 border border-red-500/30 flex items-center justify-center">
                                                      <span className="text-[8px] text-red-400 font-bold leading-none">
                                                        !
                                                      </span>
                                                    </span>
                                                    <div className="min-w-0 flex-1">
                                                      <span className="text-[9px] font-bold text-red-400 uppercase tracking-wider">
                                                        Error
                                                      </span>
                                                      <div className="mt-1">
                                                        <div className="text-[11px] whitespace-pre-wrap break-words leading-relaxed text-red-400/90 font-mono bg-red-500/[0.03] p-2 rounded-sm border border-red-500/10">
                                                          {call.output}
                                                        </div>
                                                      </div>
                                                    </div>
                                                  </div>
                                                ) : (
                                                  <>
                                                    <span className="text-[9px] font-bold text-white/15 uppercase tracking-wider">
                                                      Result
                                                    </span>
                                                    <div className="mt-1">
                                                      {renderToolOutput(call.output)}
                                                    </div>
                                                  </>
                                                )}
                                              </div>
                                            )}
                                          </div>
                                        );
                                      })}
                                  </div>
                                ))}
                              </div>

                              {/* Exchange footer */}
                              <div className="mt-6 pt-4 border-t border-white/5 flex items-center gap-4 text-[10px] font-mono text-white/30">
                                <span className="tracking-wider uppercase">Total</span>
                                {resp.totalUsage?.totalTokens != null ? (
                                  <span className="tabular-nums text-[#d19a66]">
                                    {resp.totalUsage.totalTokens} tokens
                                  </span>
                                ) : (
                                  <span className="tabular-nums">{totalTokens} tokens</span>
                                )}
                                {resp.totalUsage?.totalTokens != null &&
                                  totalTokens > 0 &&
                                  totalTokens !== resp.totalUsage.totalTokens && (
                                    <span className="tabular-nums text-white/15">
                                      (steps sum: {totalTokens})
                                    </span>
                                  )}
                                <span className="text-white/10">·</span>
                                <span className="tabular-nums">{log.duration}ms</span>
                                <span className="text-white/10">·</span>
                                <span className="tabular-nums">
                                  {steps.length} step{steps.length !== 1 ? "s" : ""}
                                </span>
                              </div>
                            </div>
                          );
                        })()}

                        {/* Child Assistant traces */}
                        {logs
                          .filter((child) => child.parent_id === log.id)
                          .map((child) => (
                            <div
                              key={child.id}
                              className="p-5 bg-[#0f1013] border-l-2 border-purple-500/30 ml-4"
                            >
                              <h3 className="text-[10px] font-bold text-white/20 uppercase tracking-[0.2em] flex items-center gap-3 mb-4">
                                <div className="w-[1px] h-3 bg-[#c678dd]" />
                                AssistANT_TRACE
                                <span
                                  className={`px-2 py-0.5 rounded-sm text-[9px] font-bold tracking-widest uppercase border ${
                                    child.status === "ERROR"
                                      ? "bg-red-500/5 text-red-400 border-red-500/20"
                                      : "bg-white/5 text-white/60 border-white/10"
                                  }`}
                                >
                                  {child.status}
                                </span>
                                <span className="text-white/30 font-mono text-[9px]">
                                  {child.duration}ms
                                </span>
                              </h3>

                              {/* Compact step timeline */}
                              {(() => {
                                let childSteps: any[] = [];
                                if (child.steps && child.steps.length > 0) {
                                  childSteps = child.steps.map(normalizeDbStep);
                                } else {
                                  try {
                                    const parsed =
                                      typeof child.response === "string"
                                        ? JSON.parse(child.response || "{}")
                                        : child.response;
                                    const rawSteps = parsed?.steps || [];
                                    childSteps = rawSteps.map(normalizeLegacyStep);
                                  } catch (e) {}
                                }
                                if (childSteps.length === 0) return null;
                                return (
                                  <div className="space-y-2">
                                    {childSteps.map((cStep: any, cIdx: number) => (
                                      <div
                                        key={cIdx}
                                        className="pl-3 border-l-2 border-white/[0.04]"
                                      >
                                        <div className="flex items-center gap-2 mb-1 text-[10px] font-mono flex-wrap">
                                          <span className="text-white/30">
                                            Step {cStep.stepNumber ?? cIdx + 1}
                                          </span>
                                          <span className="text-white/15">
                                            {cStep.finishReason}
                                          </span>
                                          {cStep.usage?.totalTokens != null && (
                                            <span className="text-white/15 tabular-nums">
                                              {cStep.usage.totalTokens} tok
                                            </span>
                                          )}
                                          {cStep.duration_ms != null && (
                                            <span className="text-white/15 tabular-nums ml-auto">
                                              {cStep.duration_ms}ms
                                            </span>
                                          )}
                                        </div>
                                        {cStep.toolCalls &&
                                          cStep.toolCalls.map((call: any, ci: number) => {
                                            const input = call.input || call.args || {};
                                            const callIsError = isToolError(call.output);
                                            return (
                                              <div
                                                key={ci}
                                                className={`mb-1 p-2 rounded-sm ${
                                                  callIsError
                                                    ? "bg-red-500/[0.04] border border-red-500/30"
                                                    : "bg-white/[0.02] border border-white/5"
                                                }`}
                                              >
                                                <div className="flex items-center justify-between">
                                                  <span className="text-[10px] font-bold text-white/30 uppercase tracking-wider flex items-center gap-2">
                                                    {readableToolName(call.toolName)}
                                                    {callIsError && (
                                                      <span className="px-1 py-[1px] rounded-sm text-[7px] font-bold uppercase tracking-widest bg-red-500/20 text-red-400 border border-red-500/30 leading-none">
                                                        Error
                                                      </span>
                                                    )}
                                                  </span>
                                                  <CopyButton
                                                    content={JSON.stringify(input, null, 2)}
                                                  />
                                                </div>
                                                <div className="mt-1">
                                                  {(() => {
                                                    const isGen =
                                                      call.toolName ===
                                                      TOOL_NAMES.GENERATE_DIALOGUE;
                                                    const isWorld =
                                                      call.toolName === TOOL_NAMES.UPDATE_ENTITY;
                                                    const isPlot =
                                                      call.toolName === TOOL_NAMES.UPDATE_PLOT;
                                                    const isCreate =
                                                      call.toolName === TOOL_NAMES.CREATE_PLOT;
                                                    const isGetAll =
                                                      call.toolName === TOOL_NAMES.LIST_ENTITIES;
                                                    const isQuery =
                                                      call.toolName === TOOL_NAMES.GET_ENTITY;
                                                    const isGetP =
                                                      call.toolName === TOOL_NAMES.GET_PLOT;

                                                    if (isGen) {
                                                      return (
                                                        <div className="text-[10px]">
                                                          <span className="text-white/30">
                                                            {(input.messages || []).length}{" "}
                                                            msgs,{" "}
                                                          </span>
                                                          <span className="text-white/30">
                                                            {(input.options || []).length} opts
                                                          </span>
                                                        </div>
                                                      );
                                                    }
                                                    if (isWorld) {
                                                      return (
                                                        <div className="text-[10px]">
                                                          <span className="text-[#d19a66] font-mono">
                                                            {input.id || "?"}
                                                          </span>
                                                          {input.shortDescription != null && (
                                                            <span className="text-white/30">
                                                              {" "}
                                                              shortDesc
                                                            </span>
                                                          )}
                                                          {input.longDescription != null && (
                                                            <span className="text-white/30">
                                                              {" "}
                                                              longDesc
                                                            </span>
                                                          )}
                                                          {input.attributes != null && (
                                                            <span className="text-white/30">
                                                              {" "}
                                                              attrs
                                                            </span>
                                                          )}
                                                          {input.opinions != null && (
                                                            <span className="text-white/30">
                                                              {" "}
                                                              opinions
                                                            </span>
                                                          )}
                                                        </div>
                                                      );
                                                    }
                                                    if (isPlot) {
                                                      return (
                                                        <div className="text-[10px]">
                                                          <span className="text-[#d19a66]">
                                                            {input.id}
                                                          </span>
                                                          <span className="text-white/20"> → </span>
                                                          <span className="text-[#98c379]">
                                                            {input.status}
                                                          </span>
                                                        </div>
                                                      );
                                                    }
                                                    if (isCreate) {
                                                      return (
                                                        <div className="text-[10px]">
                                                          <div className="text-white/50">
                                                            "{input.title}"
                                                          </div>
                                                        </div>
                                                      );
                                                    }
                                                    if (isGetAll) {
                                                      return (
                                                        <div className="text-[10px]">
                                                          <span className="text-white/30">
                                                            Fetch{" "}
                                                          </span>
                                                          <span className="text-[#d19a66]">
                                                            {input.type ? input.type : "all types"}
                                                          </span>
                                                        </div>
                                                      );
                                                    }
                                                    if (isQuery) {
                                                      return (
                                                        <div className="text-[10px]">
                                                          {input.id ? (
                                                            <>
                                                              <span className="text-white/30">
                                                                ID:{" "}
                                                              </span>
                                                              <span className="text-[#d19a66] font-mono">
                                                                {input.id}
                                                              </span>
                                                            </>
                                                          ) : input.ids ? (
                                                            <>
                                                              <span className="text-white/30">
                                                                Bulk ({input.ids.length}):{" "}
                                                              </span>
                                                              <span className="text-[#d19a66] font-mono">
                                                                [{input.ids.slice(0, 3).join(", ")}
                                                                {input.ids.length > 3
                                                                  ? `, +${input.ids.length - 3} more`
                                                                  : ""}
                                                                ]
                                                              </span>
                                                            </>
                                                          ) : input.search ? (
                                                            <>
                                                              <span className="text-white/30">
                                                                Search:{" "}
                                                              </span>
                                                              <span className="text-[#98c379]">
                                                                "{input.search}"
                                                              </span>
                                                            </>
                                                          ) : (
                                                            <span className="text-[#e06c75]">
                                                              Missing params
                                                            </span>
                                                          )}
                                                        </div>
                                                      );
                                                    }
                                                    if (isGetP) {
                                                      return (
                                                        <div className="text-[10px]">
                                                          {input.id ? (
                                                            <>
                                                              <span className="text-white/30">
                                                                Plot:{" "}
                                                              </span>
                                                              <span className="text-[#d19a66] font-mono">
                                                                {input.id}
                                                              </span>
                                                            </>
                                                          ) : input.ids ? (
                                                            <>
                                                              <span className="text-white/30">
                                                                Bulk ({input.ids.length}):{" "}
                                                              </span>
                                                              <span className="text-[#d19a66] font-mono">
                                                                [{input.ids.slice(0, 3).join(", ")}
                                                                {input.ids.length > 3
                                                                  ? `, +${input.ids.length - 3} more`
                                                                  : ""}
                                                                ]
                                                              </span>
                                                            </>
                                                          ) : input.status ? (
                                                            <>
                                                              <span className="text-white/30">
                                                                Status:{" "}
                                                              </span>
                                                              <span className="text-[#eab308]">
                                                                {input.status}
                                                              </span>
                                                            </>
                                                          ) : (
                                                            <span className="text-white/30">
                                                              All plots
                                                            </span>
                                                          )}
                                                        </div>
                                                      );
                                                    }
                                                    return (
                                                      <JsonExplorer
                                                        data={JSON.stringify(input)}
                                                        isWrapping={isWrapping}
                                                        className="max-h-[150px] overflow-auto"
                                                      />
                                                    );
                                                  })()}
                                                </div>
                                                {call.output != null && (
                                                  <div className="mt-1 pt-1 border-t border-white/5">
                                                    {callIsError ? (
                                                      <div className="flex items-start gap-1.5">
                                                        <span className="mt-[2px] shrink-0 w-3 h-3 rounded-full bg-red-500/20 border border-red-500/30 flex items-center justify-center">
                                                          <span className="text-[6px] text-red-400 font-bold leading-none">
                                                            !
                                                          </span>
                                                        </span>
                                                        <div className="min-w-0 flex-1">
                                                          <span className="text-[8px] font-bold text-red-400 uppercase tracking-wider">
                                                            Error
                                                          </span>
                                                          <div className="mt-0.5 text-[10px] whitespace-pre-wrap break-words leading-relaxed text-red-400/90 font-mono bg-red-500/[0.03] p-1.5 rounded-sm border border-red-500/10">
                                                            {call.output}
                                                          </div>
                                                        </div>
                                                      </div>
                                                    ) : (
                                                      <>{renderToolOutput(call.output, true)}</>
                                                    )}
                                                  </div>
                                                )}
                                              </div>
                                            );
                                          })}
                                      </div>
                                    ))}
                                  </div>
                                );
                              })()}

                              {/* Raw response */}
                              <div className="mt-4 pt-4 border-t border-white/5">
                                <div className="flex justify-between items-center mb-2">
                                  <span className="text-[9px] font-bold text-white/15 uppercase tracking-wider">
                                    Response
                                  </span>
                                  <CopyButton content={formatJson(child.response)} />
                                </div>
                                <JsonExplorer
                                  data={child.response}
                                  isWrapping={isWrapping}
                                  className="max-h-[200px] overflow-auto"
                                />
                              </div>
                            </div>
                          ))}

                        <div className="p-5 bg-[#0a0a0c]">
                          <div className="flex justify-between items-center mb-4">
                            <h3 className="text-[10px] font-bold text-white/20 uppercase tracking-[0.2em] flex items-center gap-3">
                              <div className="w-[1px] h-3 bg-[#98c379]" />
                              Incoming_Response
                            </h3>
                            <CopyButton content={formatJson(log.response)} />
                          </div>
                          <div
                            style={{ maxHeight: `${responseHeight}px` }}
                            className="overflow-auto debug-scrollbar"
                          >
                            <JsonExplorer
                              data={log.response}
                              isWrapping={isWrapping}
                              className=""
                            />
                          </div>
                          <div
                            className="h-6 -mx-5 mt-1 cursor-ns-resize flex items-center justify-center group/drag"
                            onMouseDown={(e) => {
                              dragRef.current = {
                                startY: e.clientY,
                                startHeight: responseHeight,
                              };
                            }}
                          >
                            <div className="h-0.5 w-12 rounded-sm bg-white/10 group-hover/drag:bg-white/30 transition-colors" />
                          </div>
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
