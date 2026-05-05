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
import { Terminal, X, Bug, Database, Monitor, GitBranch, FileText, Map, Ellipsis } from "lucide-react";
import { WorldEditor } from "@/components/debug/WorldEditor";

import { NodeGraph } from "@/components/debug/NodeGraph";
import { createDialogueConfig, createPlotConfig } from "@/components/debug/NodeGraphConfigs";
import { LlmTraceViewer } from "@/components/debug/LlmTraceViewer";
import { ConsoleViewer } from "@/components/debug/ConsoleViewer";
import { SystemPromptEditor } from "@/components/debug/SystemPromptEditor";
import { SceneViewer } from "@/components/debug/SceneViewer";

type TabId = "logs" | "console" | "world" | "graphs" | "prompt" | "scene";
type GraphMode = "dialogue" | "plot";

const TabButton: React.FC<{
  id: TabId;
  activeTab: TabId;
  onSelect: (id: TabId) => void;
  label: string;
  icon: React.ReactNode;
}> = ({ id, activeTab, onSelect, label, icon }) => (
  <button
    onClick={() => onSelect(id)}
    className={`px-4 py-3 flex items-center gap-2 border-b transition-colors ${
      activeTab === id
        ? "border-white text-white bg-white/5"
        : "border-transparent text-white/30 hover:text-white/60 hover:bg-white/2"
    }`}
  >
    {icon}
    <span className="text-[10px] font-bold uppercase tracking-[0.2em]">{label}</span>
  </button>
);

const MoreMenu: React.FC<{
  activeTab: TabId;
  onSelect: (id: TabId) => void;
}> = ({ activeTab, onSelect }) => {
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    if (open) document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, [open]);

  const isActive = activeTab === "prompt" || activeTab === "scene";

  return (
    <div ref={menuRef} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className={`px-4 py-3 flex items-center gap-2 border-b transition-colors ${
          isActive
            ? "border-white text-white bg-white/5"
            : "border-transparent text-white/30 hover:text-white/60 hover:bg-white/2"
        }`}
      >
        <Ellipsis size={14} />
        <span className="text-[10px] font-bold uppercase tracking-[0.2em]">More</span>
      </button>
      {open && (
        <div className="absolute top-full left-0 mt-1 bg-[#141414] border border-white/10 rounded-sm shadow-xl z-30 min-w-[140px]">
          <button
            onClick={() => { onSelect("prompt"); setOpen(false); }}
            className={`w-full px-4 py-2.5 flex items-center gap-2.5 text-left hover:bg-white/5 transition-colors ${
              activeTab === "prompt" ? "text-white" : "text-white/40"
            }`}
          >
            <FileText size={12} />
            <span className="text-[9px] font-bold uppercase tracking-[0.2em]">Prompt</span>
          </button>
          <button
            onClick={() => { onSelect("scene"); setOpen(false); }}
            className={`w-full px-4 py-2.5 flex items-center gap-2.5 text-left hover:bg-white/5 transition-colors ${
              activeTab === "scene" ? "text-white" : "text-white/40"
            }`}
          >
            <Map size={12} />
            <span className="text-[9px] font-bold uppercase tracking-[0.2em]">Scene</span>
          </button>
        </div>
      )}
    </div>
  );
};

export const DebugPanel: React.FC<{
  onJumpToReplay?: (stepId: string) => void;
  currentReplayStepId?: string | null;
}> = ({ onJumpToReplay, currentReplayStepId }) => {
  const [isOpen, setIsOpen] = useState(false);
  const [activeTab, setActiveTab] = useState<TabId>("logs");
  const [panelWidth, setPanelWidth] = useState(640);
  const [graphMode, setGraphMode] = useState<GraphMode>("dialogue");
  const panelDragRef = useRef<{ startX: number; startWidth: number } | null>(null);

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
    [currentReplayStepId],
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
              className="fixed inset-y-0 right-0 bg-[#0a0a0a] text-gray-100 shadow-2xl z-50 flex flex-col border-l border-white/10 font-mono text-sm"
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
              <div className="flex items-center justify-between px-4 border-b border-white/10 bg-[#0a0a0a]/90 backdrop-blur-md sticky top-0 z-10">
                <div className="flex items-center">
                  <TabButton
                    id="logs"
                    activeTab={activeTab}
                    onSelect={setActiveTab}
                    label="Logs"
                    icon={<Terminal size={14} />}
                  />
                  <TabButton
                    id="console"
                    activeTab={activeTab}
                    onSelect={setActiveTab}
                    label="Console"
                    icon={<Monitor size={14} />}
                  />
                  <TabButton
                    id="world"
                    activeTab={activeTab}
                    onSelect={setActiveTab}
                    label="World"
                    icon={<Database size={14} />}
                  />
                  <TabButton
                    id="graphs"
                    activeTab={activeTab}
                    onSelect={setActiveTab}
                    label="Graphs"
                    icon={<GitBranch size={14} />}
                  />
                  <MoreMenu activeTab={activeTab} onSelect={setActiveTab} />
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
                {activeTab === "logs" && <LlmTraceViewer />}
                {activeTab === "console" && <ConsoleViewer />}
                {activeTab === "world" && <WorldEditor />}
                {activeTab === "graphs" && (
                  <div className="flex flex-col h-full">
                    <div className="flex items-center gap-1 mb-4 flex-shrink-0">
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
                    </div>
                    <div className="flex-1 min-h-0">
                      {graphMode === "dialogue" ? (
                        <NodeGraph key="dialogue" config={dialogueConfig} />
                      ) : (
                        <NodeGraph key="plot" config={plotConfig} />
                      )}
                    </div>
                  </div>
                )}
                {activeTab === "prompt" && <SystemPromptEditor />}
                {activeTab === "scene" && <SceneViewer />}
              </div>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </>
  );
};
