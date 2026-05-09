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

import React, { useState, useEffect } from "react";
import { motion } from "motion/react";
import { GitBranch, X, Save, AlertCircle, ChevronRight } from "lucide-react";
import type { Message, DialogueOption } from "@/types/dialogue";
import { CustomSelect, ResizableTextarea } from "./shared";
import type { NodeGraphConfig, NodeRenderProps, InspectorProps } from "./NodeGraph";

// ═══════════════════════════════════════════════════════════════════════════
// Dialogue Tree Config
// ═══════════════════════════════════════════════════════════════════════════

interface StepData {
  id: string;
  parentStepId: string | null;
  parentOptionId: string | null;
  messages: Message[];
  options: DialogueOption[];
  isActive: boolean;
  createdAt: string;
}

interface TreeData {
  root: StepData | null;
  steps: Record<string, StepData>;
  leafIds: string[];
  stats: { totalSteps: number; rootId: string | null; leafIds: string[]; branchCount: number };
}

const DIALOGUE_NODE_W = 200;
const DIALOGUE_NODE_H = 110;

const SPEAKER_TYPES = [
  "YOU",
  "CHARACTER",
  "INNER_VOICE",
  "SYSTEM",
  "ROLL",
  "NOTIFICATION",
] as const;

const TYPE_COLOR: Record<string, string> = {
  YOU: "rgba(255,255,255,0.6)",
  CHARACTER: "#61afef",
  INNER_VOICE: "#c678dd",
  SYSTEM: "rgba(255,255,255,0.25)",
  ROLL: "#eab308",
  NOTIFICATION: "#98c379",
};

// ── Dialogue node card ────────────────────────────────────────────────────

const DialogueNodeCard: React.FC<NodeRenderProps<StepData> & { isCurrentReplay: boolean }> = ({
  node,
  pos,
  isSelected,
  isLeaf,
  isRoot,
  isEffectivelyActive,
  isCurrentReplay,
}) => {
  const firstMsg = node.messages?.[0];
  const preview = firstMsg
    ? firstMsg.text.slice(0, 42) + (firstMsg.text.length > 42 ? "…" : "")
    : "(no messages)";
  const shortId = node.id.slice(0, 12) + (node.id.length > 12 ? "…" : "");

  let borderColor = "rgba(255,255,255,0.12)";
  let bgColor = "rgba(255,255,255,0.025)";
  if (!isEffectivelyActive) {
    borderColor = "rgba(255,255,255,0.05)";
    bgColor = "rgba(255,255,255,0.01)";
  } else if (isCurrentReplay) {
    borderColor = "rgba(52,211,153,0.7)";
    bgColor = "rgba(52,211,153,0.07)";
  } else if (isSelected) {
    borderColor = "rgba(255,255,255,0.55)";
    bgColor = "rgba(255,255,255,0.07)";
  } else if (isLeaf) {
    borderColor = "rgba(152,195,121,0.5)";
    bgColor = "rgba(152,195,121,0.04)";
  } else if (isRoot) {
    borderColor = "rgba(255,107,53,0.35)";
    bgColor = "rgba(255,107,53,0.04)";
  }

  return (
    <div
      style={{
        position: "absolute",
        left: pos.x,
        top: pos.y,
        width: DIALOGUE_NODE_W,
        height: DIALOGUE_NODE_H,
        border: `1px solid ${borderColor}`,
        background: bgColor,
        opacity: isEffectivelyActive ? 1 : 0.35,
        transition: "border-color 0.2s, background 0.2s",
        cursor: "pointer",
        userSelect: "none",
        borderRadius: "2px",
        boxSizing: "border-box",
        display: "flex",
        flexDirection: "column",
      }}
      className={isLeaf && isEffectivelyActive ? "leaf-pulse" : ""}
    >
      {/* Header */}
      <div
        className="px-2.5 py-1.5 flex items-center justify-between border-b flex-shrink-0"
        style={{ borderColor: "rgba(255,255,255,0.06)" }}
      >
        <span className="text-[9px] font-mono text-white/30 truncate">{shortId}</span>
        <div className="flex items-center gap-1.5 flex-shrink-0 ml-1">
          {isRoot && (
            <span className="text-[8px] font-bold uppercase tracking-wider text-accent/60 border border-accent/20 px-1 rounded-sm">
              ROOT
            </span>
          )}
          {isCurrentReplay && (
            <span className="text-[8px] font-bold uppercase tracking-wider text-emerald-400/80 border border-emerald-400/30 px-1 rounded-sm">
              NOW
            </span>
          )}
          {!isEffectivelyActive && (
            <span className="text-[8px] font-bold uppercase tracking-wider text-white/20 border border-white/8 px-1 rounded-sm">
              DEAD
            </span>
          )}
          {isLeaf && isEffectivelyActive && (
            <span className="text-[8px] font-bold uppercase tracking-wider text-[#98c379]/70 border border-[#98c379]/20 px-1 rounded-sm">
              LEAF
            </span>
          )}
        </div>
      </div>
      {/* Preview */}
      <div className="px-2.5 py-1.5 flex-1 min-h-0">
        <p
          className="text-[10px] leading-snug italic line-clamp-2"
          style={{
            color: isEffectivelyActive ? "rgba(255,255,255,0.45)" : "rgba(255,255,255,0.2)",
          }}
        >
          {preview}
        </p>
      </div>
      {/* Footer badges */}
      <div
        className="px-2.5 py-1.5 flex items-center gap-2 border-t flex-shrink-0"
        style={{ borderColor: "rgba(255,255,255,0.04)" }}
      >
        <span className="text-[9px] font-mono text-white/20">
          {node.messages?.length ?? 0} msg{(node.messages?.length ?? 0) !== 1 ? "s" : ""}
        </span>
        <span className="text-white/10">·</span>
        <span className="text-[9px] font-mono text-white/20">
          {node.options?.length ?? 0} opt{(node.options?.length ?? 0) !== 1 ? "s" : ""}
        </span>
      </div>
    </div>
  );
};

// ── Dialogue inspector ────────────────────────────────────────────────────

const DialogueInspector: React.FC<
  InspectorProps<StepData> & { onJumpToReplay?: (stepId: string) => void }
> = ({
  node,
  isLeaf,
  isEffectivelyActive,
  onClose,
  onSaved,
  onJumpToReplay,
  height,
  onResizeStart,
}) => {
  const [messages, setMessages] = useState<Message[]>(node.messages ?? []);
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [savedFlash, setSavedFlash] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setMessages(node.messages ?? []);
    setEditingIdx(null);
    setError(null);
  }, [node.id]);

  const save = async () => {
    setIsSaving(true);
    setError(null);
    try {
      const res = await fetch(`/api/dialogue/${node.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ messages, options: node.options ?? [] }),
      });
      if (!res.ok) {
        setError(await res.text());
        return;
      }
      setSavedFlash(true);
      setTimeout(() => setSavedFlash(false), 1500);
      onSaved();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSaving(false);
    }
  };

  const updateMsg = (idx: number, field: keyof Message, val: string) => {
    setMessages((prev) => prev.map((m, i) => (i === idx ? { ...m, [field]: val } : m)));
  };

  return (
    <motion.div
      initial={{ y: 8, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      exit={{ y: 8, opacity: 0 }}
      transition={{ duration: 0.15 }}
      className="flex-shrink-0 bg-[#0c0c0e] relative"
      style={{ height }}
    >
      <div
        onMouseDown={onResizeStart}
        className="absolute top-0 left-0 right-0 h-3 flex items-center justify-center cursor-ns-resize border-t border-white/10 group/rhandle z-10"
      >
        <div className="w-10 h-0.5 bg-white/15 rounded-full group-hover/rhandle:bg-white/40 transition-colors" />
      </div>

      <div className="flex flex-col h-full" style={{ paddingTop: "12px" }}>
        <div className="flex-shrink-0 flex items-center justify-between px-4 py-2 border-b border-white/6">
          <div className="flex items-center gap-3">
            <span className="text-[9px] font-mono text-white/30">{node.id}</span>
            {isLeaf && (
              <span className="text-[8px] font-bold uppercase tracking-wider text-[#98c379]/60 border border-[#98c379]/20 px-1 rounded-sm">
                LEAF
              </span>
            )}
            {!isEffectivelyActive && (
              <span className="text-[8px] font-bold uppercase tracking-wider text-white/20 border border-white/10 px-1 rounded-sm">
                INACTIVE
              </span>
            )}
            <span className="text-[9px] text-white/20 font-mono">
              {node.messages?.length ?? 0} msgs · {node.options?.length ?? 0} opts
            </span>
          </div>
          <div className="flex items-center gap-2">
            {onJumpToReplay && isEffectivelyActive && (
              <button
                onClick={() => onJumpToReplay(node.id)}
                className="flex items-center gap-1.5 px-2.5 py-1 bg-emerald-500/8 border border-emerald-500/20 text-emerald-400/80 text-[9px] font-bold uppercase tracking-wider rounded-sm hover:bg-emerald-500/15 transition-colors"
              >
                <GitBranch size={10} />
                Jump to Replay
              </button>
            )}
            <button
              onClick={save}
              disabled={isSaving}
              className={`flex items-center gap-1.5 px-2.5 py-1 rounded-sm border text-[9px] font-bold uppercase tracking-wider transition-all disabled:opacity-50 ${
                savedFlash
                  ? "bg-[#98c379]/15 text-[#98c379] border-[#98c379]/25"
                  : "bg-white/5 text-white/40 border-white/10 hover:text-white hover:bg-white/8"
              }`}
            >
              <Save size={10} />
              {isSaving ? "Saving…" : savedFlash ? "Saved ✓" : "Save"}
            </button>
            <button
              onClick={onClose}
              className="p-1 text-white/20 hover:text-white/50 transition-colors"
            >
              <X size={12} />
            </button>
          </div>
        </div>

        {error && (
          <div className="flex-shrink-0 mx-3 mt-2 p-2 bg-red-500/10 border border-red-500/20 rounded-sm flex items-center gap-2 text-red-400 text-[10px]">
            <AlertCircle size={10} />
            {error}
          </div>
        )}

        <div className="flex flex-1 overflow-hidden">
          {/* Messages list */}
          <div className="flex-1 overflow-y-auto debug-scrollbar px-3 py-2 space-y-1 border-r border-white/6">
            <div className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 mb-2">
              Messages
            </div>
            {messages.map((msg, idx) => (
              <div key={idx}>
                {editingIdx === idx ? (
                  <div className="p-2 bg-white/[0.04] border border-white/12 rounded-sm space-y-1.5">
                    <div className="flex items-center gap-1.5">
                      <CustomSelect
                        value={msg.type}
                        options={[...SPEAKER_TYPES]}
                        onChange={(v) => updateMsg(idx, "type", v)}
                        className="flex-1"
                      />
                      <input
                        className="flex-1 bg-white/[0.04] border border-white/8 rounded-sm px-1.5 py-0.5 text-[10px] font-mono text-white/60 focus:outline-none"
                        placeholder="speaker"
                        value={msg.speaker}
                        onChange={(e) => updateMsg(idx, "speaker", e.target.value)}
                      />
                      <button
                        onClick={() => setEditingIdx(null)}
                        className="flex-shrink-0 w-5 h-5 flex items-center justify-center text-white/20 hover:text-white/60 rounded-sm transition-colors"
                      >
                        <X size={8} />
                      </button>
                    </div>
                    <ResizableTextarea
                      autoFocus
                      className="w-full bg-white/[0.04] border border-white/8 rounded-sm px-1.5 py-1 text-[10px] font-mono text-white/70 focus:outline-none"
                      initialHeight={80}
                      minHeight={36}
                      value={msg.text}
                      onChange={(e) => updateMsg(idx, "text", e.target.value)}
                      onKeyDown={(e) => e.key === "Escape" && setEditingIdx(null)}
                    />
                  </div>
                ) : (
                  <button
                    onClick={() => setEditingIdx(idx)}
                    className="w-full text-left py-1 px-1 rounded-sm hover:bg-white/[0.03] transition-colors group/item"
                  >
                    <span
                      className="text-[8px] font-bold uppercase tracking-wider mr-2"
                      style={{ color: TYPE_COLOR[msg.type] ?? "rgba(255,255,255,0.4)" }}
                    >
                      {msg.speaker}
                    </span>
                    <span className="text-[10px] text-white/40 group-hover/item:text-white/60 transition-colors">
                      {msg.text.slice(0, 48)}
                      {msg.text.length > 48 ? "…" : ""}
                    </span>
                  </button>
                )}
              </div>
            ))}
          </div>
          {/* Options list */}
          <div className="w-56 flex-shrink-0 overflow-y-auto debug-scrollbar px-3 py-2 space-y-1">
            <div className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 mb-2">
              Options
            </div>
            {(node.options ?? []).map((opt) => (
              <div key={opt.id} className="py-1">
                <div className="flex items-start gap-1.5">
                  <ChevronRight size={9} className="text-white/20 flex-shrink-0 mt-0.5" />
                  <div className="min-w-0">
                    <p className="text-[10px] text-white/50 leading-snug truncate">{opt.text}</p>
                    {opt.nextStepId ? (
                      <span className="text-[8px] font-mono text-[#98c379]/50">
                        → {opt.nextStepId.slice(0, 8)}…
                      </span>
                    ) : (
                      <span className="text-[8px] font-mono text-white/15">unexplored</span>
                    )}
                    {opt.check && (
                      <span className="ml-1 text-[8px] font-bold text-[#eab308]/50">
                        [{opt.check.skill.toUpperCase()}]
                      </span>
                    )}
                  </div>
                </div>
              </div>
            ))}
            {!node.options?.length && <p className="text-[9px] text-white/15 italic">No options</p>}
          </div>
        </div>
      </div>
    </motion.div>
  );
};

// ── Config factory ────────────────────────────────────────────────────────

export function createDialogueConfig(opts: {
  onJumpToReplay?: (stepId: string) => void;
  currentStepId?: string | null;
}): NodeGraphConfig<StepData> {
  let treeLeafIds: Set<string> | null = null;

  return {
    nodeWidth: DIALOGUE_NODE_W,
    nodeHeight: DIALOGUE_NODE_H,
    title: "Dialogue_Tree",
    emptyLabel: "No_Dialogue_Tree",
    icon: <GitBranch size={14} />,
    getParentId: (node) => node.parentStepId,
    getSortKey: (node) => node.createdAt ?? "",
    getStatsLabel: ({ total, branches, activeLeaves }) =>
      `${total} steps · ${branches} branches · ${activeLeaves} leaves`,
    fetchData: async () => {
      const res = await fetch("/api/dialogue/tree");
      if (!res.ok) return [];
      const data: TreeData = await res.json();
      treeLeafIds = new Set(data.leafIds ?? []);
      return Object.values(data.steps ?? {});
    },
    isEffectivelyActive: (id, nodeMap) => {
      let cur: StepData | undefined = nodeMap[id];
      while (cur) {
        if (!cur.isActive) return false;
        cur = cur.parentStepId ? nodeMap[cur.parentStepId] : undefined;
      }
      return true;
    },
    isLeaf: (id) => treeLeafIds?.has(id) ?? false,
    getRootId: (nodes) => {
      const root = nodes.find((n) => n.parentStepId === null);
      return root?.id ?? null;
    },
    getEdgeLabel: (childId, parentId, nodeMap) => {
      const parent = nodeMap[parentId] as StepData | undefined;
      const child = nodeMap[childId] as StepData | undefined;
      const option = parent?.options.find((o) => o.id === child?.parentOptionId);
      return option ? option.text.slice(0, 22) + (option.text.length > 22 ? "…" : "") : "";
    },
    renderNode: (props) => (
      <DialogueNodeCard
        {...props}
        isCurrentReplay={props.node.id === (opts.currentStepId ?? null)}
      />
    ),
    renderInspector: (props) => (
      <DialogueInspector {...props} onJumpToReplay={opts.onJumpToReplay} />
    ),
  };
}
