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

import React, { useState, useEffect, useRef, useMemo, useCallback } from "react";
import { motion, AnimatePresence } from "motion/react";
import {
  Terminal,
  X,
  Bug,
  Database,
  GitBranch,
  FileText,
  Map,
  StickyNote,
  GripVertical,
  Loader2,
  Sparkles,
} from "lucide-react";
import { WorldEditor } from "@/components/debug/WorldEditor";

import { NodeGraph } from "@/components/debug/NodeGraph";
import { createDialogueConfig } from "@/components/debug/DialogueConfig";
import { createPlotConfig } from "@/components/debug/PlotConfig";
import { LlmTraceViewer } from "@/components/debug/LlmTraceViewer";
import { SystemPromptEditor } from "@/components/debug/SystemPromptEditor";
import { SceneViewer } from "@/components/debug/SceneViewer";
import { FactsViewer } from "@/components/debug/FactsViewer";
import { CustomSelect } from "@/components/debug/shared";

type TabId = "logs" | "world" | "graphs" | "prompt" | "scene" | "facts";
type GraphMode = "dialogue" | "plot";

const TAB_DEFS: Record<TabId, { label: string; icon: React.ReactNode }> = {
  logs: { label: "Logs", icon: <Terminal size={14} /> },
  world: { label: "World", icon: <Database size={14} /> },
  graphs: { label: "Graphs", icon: <GitBranch size={14} /> },
  prompt: { label: "Prompt", icon: <FileText size={14} /> },
  scene: { label: "Scene", icon: <Map size={14} /> },
  facts: { label: "Facts", icon: <StickyNote size={14} /> },
};

const DEFAULT_TAB_ORDER: TabId[] = ["logs", "world", "graphs", "prompt", "scene", "facts"];

export const DebugPanel: React.FC<{
  onJumpToReplay?: (stepId: string) => void;
  currentReplayStepId?: string | null;
}> = ({ onJumpToReplay, currentReplayStepId }) => {
  const [isOpen, setIsOpen] = useState(false);
  const [activeTab, setActiveTab] = useState<TabId>("logs");
  const [panelWidth, setPanelWidth] = useState(640);
  const [graphMode, setGraphMode] = useState<GraphMode>("dialogue");
  const [tabOrder, setTabOrder] = useState<TabId[]>(DEFAULT_TAB_ORDER);
  const [dragTabId, setDragTabId] = useState<TabId | null>(null);
  const [dragOverTabId, setDragOverTabId] = useState<TabId | null>(null);
  const panelDragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [pregenSize, setPregenSize] = useState(10);
  const [pregenerating, setPregenerating] = useState(false);
  const [pregenError, setPregenError] = useState<string | null>(null);
  const [pregenVersion, setPregenVersion] = useState(0);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!panelDragRef.current) return;
      const delta = panelDragRef.current.startX - e.clientX;
      const newWidth = Math.max(360, Math.min(1400, panelDragRef.current.startWidth + delta));
      setPanelWidth(newWidth);
    };
    const onUp = () => {
      panelDragRef.current = null;
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  const handleJumpToReplay = useCallback(
    (stepId: string) => {
      setIsOpen(false);
      onJumpToReplay?.(stepId);
    },
    [onJumpToReplay],
  );

  const handlePregeneratePlots = useCallback(async () => {
    setPregenerating(true);
    setPregenError(null);
    try {
      const res = await fetch("/api/plots/pregen", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ size: pregenSize }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({ error: res.statusText }));
        throw new Error(body.error || `HTTP ${res.status}`);
      }
      setPregenVersion((v) => v + 1);
    } catch (e: unknown) {
      setPregenError(e instanceof Error ? e.message : String(e));
    } finally {
      setPregenerating(false);
    }
  }, [pregenSize]);

  const dialogueConfig = useMemo(
    () =>
      createDialogueConfig({
        onJumpToReplay: handleJumpToReplay,
        currentStepId: currentReplayStepId,
      }),
    [handleJumpToReplay, currentReplayStepId],
  );

  const plotConfig = useMemo(
    () =>
      createPlotConfig({
        isReplayActive: !!currentReplayStepId,
        currentReplayStepId: currentReplayStepId ?? null,
      }),
    [currentReplayStepId, pregenVersion],
  );

  return (
    <>
      <button
        id="debug-panel-toggle"
        onClick={() => setIsOpen(true)}
        className="fixed bottom-8 right-8 z-50 h-11 min-w-[2.75rem] px-3 bg-surface-card border border-accent/30 rounded-full text-accent hover:bg-accent hover:text-white transition-all duration-300 shadow-lg group flex items-center justify-center overflow-hidden"
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
              className="fixed inset-0 bg-black/50 backdrop-blur-sm z-40"
            />
            <motion.div
              id="debug-panel-modal"
              initial={{ opacity: 0, x: 300 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 300 }}
              className="fixed inset-y-0 right-0 bg-surface text-gray-100 shadow-2xl z-50 flex flex-col border-l border-white/10 font-mono text-sm"
              style={{ width: `${panelWidth}px`, maxWidth: "100%" }}
            >
              {/* Left-edge resize handle */}
              <div
                className="hidden md:flex absolute left-0 top-0 bottom-0 w-2 -ml-1 cursor-ew-resize items-center justify-center group/panel z-20"
                onMouseDown={(e) => {
                  panelDragRef.current = { startX: e.clientX, startWidth: panelWidth };
                }}
              >
                <div className="w-0.5 h-12 rounded-sm bg-white/5 group-hover/panel:bg-white/20 transition-colors" />
              </div>
              <div className="flex items-center justify-between px-4 border-b border-white/10 bg-surface/90 backdrop-blur-md sticky top-0 z-10">
                <div className="flex items-center">
                  {tabOrder.map((id) => {
                    const def = TAB_DEFS[id];
                    const handleDrop = () => {
                      if (dragTabId === null || dragTabId === id) return;
                      setTabOrder((prev) => {
                        const from = prev.indexOf(dragTabId);
                        const to = prev.indexOf(id);
                        const next = [...prev];
                        next.splice(from, 1);
                        next.splice(to, 0, dragTabId);
                        return next;
                      });
                      setDragTabId(null);
                      setDragOverTabId(null);
                    };
                    return (
                      <button
                        key={id}
                        draggable
                        onClick={() => setActiveTab(id)}
                        onDragStart={() => setDragTabId(id)}
                        onDragOver={(e) => {
                          e.preventDefault();
                          if (dragTabId !== id) setDragOverTabId(id);
                        }}
                        onDrop={handleDrop}
                        onDragEnd={() => {
                          setDragTabId(null);
                          setDragOverTabId(null);
                        }}
                        className={`relative group/tab px-3 py-3 flex items-center gap-2 border-b transition-colors ${
                          activeTab === id
                            ? "border-white text-white bg-white/5"
                            : "border-transparent text-white/30 hover:text-white/60 hover:bg-white/2"
                        } ${dragTabId === id ? "opacity-30" : "opacity-100"} ${
                          dragOverTabId === id && dragTabId !== id
                            ? "border-l-2 border-l-accent/60"
                            : ""
                        }`}
                      >
                        <span className="absolute left-0.5 top-1/2 -translate-y-1/2 opacity-0 group-hover/tab:opacity-100 transition-opacity text-white/15 cursor-grab active:cursor-grabbing">
                          <GripVertical size={10} />
                        </span>
                        {def.icon}
                        <span className="text-[10px] font-bold uppercase tracking-[0.2em]">
                          {def.label}
                        </span>
                      </button>
                    );
                  })}
                </div>
                <div className="flex items-center gap-3 pr-4">
                  <button
                    onClick={() => setIsOpen(false)}
                    className="p-1.5 hover:bg-white/5 rounded-sm text-white/40 hover:text-white transition-colors focus-visible:ring-1 focus-visible:ring-accent/50"
                  >
                    <X size={18} />
                  </button>
                </div>
              </div>

              <div className="flex-1 p-6 min-h-0 flex flex-col">
                {activeTab === "logs" && <LlmTraceViewer />}
                {activeTab === "world" && <WorldEditor />}
                {activeTab === "graphs" && (
                  <div className="flex flex-col h-full">
                    <div className="flex items-center gap-1 mb-4 flex-shrink-0 flex-wrap">
                      <button
                        onClick={() => setGraphMode("dialogue")}
                        className={`px-3 py-1.5 text-[9px] font-bold uppercase tracking-[0.15em] border rounded-sm transition-colors ${
                          graphMode === "dialogue"
                            ? "text-white border-white/20 bg-white/5"
                            : "text-white/30 border-transparent hover:text-white/50 hover:bg-white/2"
                        }`}
                      >
                        Dialogue
                      </button>
                      <button
                        onClick={() => setGraphMode("plot")}
                        className={`px-3 py-1.5 text-[9px] font-bold uppercase tracking-[0.15em] border rounded-sm transition-colors ${
                          graphMode === "plot"
                            ? "text-white border-white/20 bg-white/5"
                            : "text-white/30 border-transparent hover:text-white/50 hover:bg-white/2"
                        }`}
                      >
                        Plots
                      </button>
                      {graphMode === "plot" && (
                        <>
                          <span className="text-white/10 mx-1">|</span>
                          <CustomSelect
                            value={String(pregenSize)}
                            options={[
                              { value: "5", label: "5 nodes" },
                              { value: "10", label: "10 nodes" },
                              { value: "15", label: "15 nodes" },
                              { value: "20", label: "20 nodes" },
                              { value: "30", label: "30 nodes" },
                            ]}
                            onChange={(v) => setPregenSize(Number(v))}
                            className="w-[90px] min-w-0"
                          />
                          <button
                            onClick={handlePregeneratePlots}
                            disabled={pregenerating}
                            className="flex items-center gap-1.5 px-2.5 py-1.5 bg-accent/10 text-accent hover:bg-accent/20 rounded-sm border border-accent/20 text-[9px] font-bold uppercase tracking-wider transition-all disabled:opacity-50"
                          >
                            {pregenerating ? (
                              <Loader2 size={11} className="animate-spin" />
                            ) : (
                              <Sparkles size={11} />
                            )}
                            Pre-generate
                          </button>
                          {pregenError && (
                            <span className="text-[9px] font-mono text-red-400/80 ml-1">
                              {pregenError}
                            </span>
                          )}
                        </>
                      )}
                    </div>
                    <div className="flex-1 min-h-0">
                      {graphMode === "dialogue" ? (
                        <NodeGraph key="dialogue" config={dialogueConfig} />
                      ) : (
                        <NodeGraph key={`plot-v${pregenVersion}`} config={plotConfig} />
                      )}
                    </div>
                  </div>
                )}
                {activeTab === "prompt" && <SystemPromptEditor />}
                {activeTab === "scene" && <SceneViewer />}
                {activeTab === "facts" && <FactsViewer />}
              </div>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </>
  );
};
