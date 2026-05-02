import React, { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "motion/react";
import { Terminal, X, Bug, MessageSquare, Database, Monitor, GitBranch } from "lucide-react";
import { WorldEditor } from "@/components/debug/WorldEditor";
import { HistoryEditor } from "@/components/debug/HistoryEditor";
import { DialogueTreeGraph } from "@/components/debug/DialogueTreeGraph";
import { LlmTraceViewer } from "@/components/debug/LlmTraceViewer";
import { ConsoleViewer } from "@/components/debug/ConsoleViewer";

export const DebugPanel: React.FC<{
  onJumpToReplay?: (stepId: string) => void;
  currentReplayStepId?: string | null;
}> = ({ onJumpToReplay, currentReplayStepId }) => {
  const [isOpen, setIsOpen] = useState(false);
  const [activeTab, setActiveTab] = useState<"logs" | "console" | "history" | "world" | "tree">(
    "logs",
  );
  const [panelWidth, setPanelWidth] = useState(640);
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

  const TabButton: React.FC<{
    id: "logs" | "console" | "history" | "world" | "tree";
    label: string;
    icon: React.ReactNode;
  }> = ({ id, label, icon }) => (
    <button
      onClick={() => setActiveTab(id)}
      className={`px-4 py-3 flex items-center gap-2 border-b transition-all ${
        activeTab === id
          ? "border-white text-white bg-white/5"
          : "border-transparent text-white/30 hover:text-white/60 hover:bg-white/2"
      }`}
    >
      {icon}
      <span className="text-[10px] font-bold uppercase tracking-[0.2em]">{label}</span>
    </button>
  );

  const handleJumpToReplay = (stepId: string) => {
    setIsOpen(false);
    onJumpToReplay?.(stepId);
  };

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
                  <TabButton id="logs" label="Logs" icon={<Terminal size={14} />} />
                  <TabButton id="console" label="Console" icon={<Monitor size={14} />} />
                  <TabButton id="history" label="History" icon={<MessageSquare size={14} />} />
                  <TabButton id="world" label="World" icon={<Database size={14} />} />
                  <TabButton id="tree" label="Tree" icon={<GitBranch size={14} />} />
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
                {activeTab === "history" && <HistoryEditor />}
                {activeTab === "world" && <WorldEditor />}
                {activeTab === "tree" && (
                  <DialogueTreeGraph
                    onJumpToReplay={handleJumpToReplay}
                    currentStepId={currentReplayStepId}
                  />
                )}
              </div>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </>
  );
};
