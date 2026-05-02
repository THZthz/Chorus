import React, { useState, useEffect, useRef, useCallback } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  Maximize2,
  RotateCcw,
  RefreshCw,
  X,
  Save,
  AlertCircle,
  GitBranch,
  ChevronRight,
} from "lucide-react";
import { Message, DialogueOption } from "@/types/dialogue";
import { CustomSelect, ResizableTextarea } from "./shared";

// ── Types ──────────────────────────────────────────────────────────────────

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

interface NodePos {
  id: string;
  x: number;
  y: number;
}

// ── Layout constants ───────────────────────────────────────────────────────

const NODE_W = 200;
const NODE_H = 90;
const H_GAP = 52;
const V_GAP = 96;

// ── Layout algorithm ────────────────────────────────────────────────────────

function computeLayout(steps: Record<string, StepData>): NodePos[] {
  const childrenMap = new Map<string | null, string[]>();
  for (const [id, step] of Object.entries(steps)) {
    const parent = step.parentStepId ?? null;
    if (!childrenMap.has(parent)) childrenMap.set(parent, []);
    childrenMap.get(parent)!.push(id);
  }
  // Sort children by creation time for stable layout
  for (const [, children] of childrenMap) {
    children.sort((a, b) => (steps[a]?.createdAt ?? "").localeCompare(steps[b]?.createdAt ?? ""));
  }

  function subtreeWidth(id: string): number {
    const children = childrenMap.get(id) ?? [];
    if (children.length === 0) return NODE_W;
    const total =
      children.reduce((sum, c) => sum + subtreeWidth(c), 0) + H_GAP * (children.length - 1);
    return Math.max(NODE_W, total);
  }

  const positions: NodePos[] = [];
  function place(id: string, cx: number, cy: number) {
    positions.push({ id, x: cx - NODE_W / 2, y: cy });
    const children = childrenMap.get(id) ?? [];
    if (children.length === 0) return;
    const totalW =
      children.reduce((sum, c) => sum + subtreeWidth(c), 0) + H_GAP * (children.length - 1);
    let x = cx - totalW / 2;
    for (const child of children) {
      const sw = subtreeWidth(child);
      place(child, x + sw / 2, cy + NODE_H + V_GAP);
      x += sw + H_GAP;
    }
  }

  const roots = childrenMap.get(null) ?? [];
  let startX = 0;
  for (const root of roots) {
    const sw = subtreeWidth(root);
    place(root, startX + sw / 2, 0);
    startX += sw + H_GAP;
  }
  return positions;
}

// ── SVG edge path ────────────────────────────────────────────────────────────

function edgePath(sx: number, sy: number, ex: number, ey: number): string {
  const my = (sy + ey) / 2;
  return `M ${sx} ${sy} C ${sx} ${my}, ${ex} ${my}, ${ex} ${ey}`;
}

// ── Node component ─────────────────────────────────────────────────────────

const GraphNode: React.FC<{
  step: StepData;
  pos: NodePos;
  isSelected: boolean;
  isLeaf: boolean;
  isRoot: boolean;
  isEffectivelyActive: boolean;
  onClick: () => void;
}> = ({ step, pos, isSelected, isLeaf, isRoot, isEffectivelyActive }) => {
  const firstMsg = step.messages[0];
  const preview = firstMsg
    ? firstMsg.text.slice(0, 42) + (firstMsg.text.length > 42 ? "…" : "")
    : "(no messages)";
  const shortId = step.id.slice(0, 12) + (step.id.length > 12 ? "…" : "");

  let borderColor = "rgba(255,255,255,0.12)";
  let bgColor = "rgba(255,255,255,0.025)";
  if (!isEffectivelyActive) {
    borderColor = "rgba(255,255,255,0.05)";
    bgColor = "rgba(255,255,255,0.01)";
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
        width: NODE_W,
        height: NODE_H,
        border: `1px solid ${borderColor}`,
        background: bgColor,
        opacity: isEffectivelyActive ? 1 : 0.35,
        transition: "border-color 0.2s, background 0.2s",
        cursor: "pointer",
        userSelect: "none",
        borderRadius: "2px",
        boxSizing: "border-box",
      }}
      className={isLeaf && isEffectivelyActive ? "leaf-pulse" : ""}
    >
      {/* Header */}
      <div
        className="px-2.5 py-1.5 flex items-center justify-between border-b"
        style={{ borderColor: "rgba(255,255,255,0.06)" }}
      >
        <span className="text-[9px] font-mono text-white/30 truncate">{shortId}</span>
        <div className="flex items-center gap-1.5 flex-shrink-0 ml-1">
          {isRoot && (
            <span className="text-[8px] font-bold uppercase tracking-wider text-[#ff6b35]/60 border border-[#ff6b35]/20 px-1 rounded-sm">
              ROOT
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
      <div className="px-2.5 py-1.5">
        <p
          className="text-[10px] leading-snug italic"
          style={{
            color: isEffectivelyActive ? "rgba(255,255,255,0.45)" : "rgba(255,255,255,0.2)",
          }}
        >
          {preview}
        </p>
      </div>
      {/* Footer badges */}
      <div
        className="absolute bottom-0 left-0 right-0 px-2.5 py-1 flex items-center gap-2 border-t"
        style={{ borderColor: "rgba(255,255,255,0.04)" }}
      >
        <span className="text-[9px] font-mono text-white/20">
          {step.messages.length} msg{step.messages.length !== 1 ? "s" : ""}
        </span>
        <span className="text-white/10">·</span>
        <span className="text-[9px] font-mono text-white/20">
          {step.options.length} opt{step.options.length !== 1 ? "s" : ""}
        </span>
      </div>
    </div>
  );
};

// ── Inspector panel ──────────────────────────────────────────────────────────

const SPEAKER_TYPES = [
  "YOU",
  "CHARACTER",
  "INNER_VOICE",
  "SYSTEM",
  "ROLL",
  "NOTIFICATION",
] as const;

const Inspector: React.FC<{
  step: StepData;
  isLeaf: boolean;
  isEffectivelyActive: boolean;
  onClose: () => void;
  onSave: (id: string, messages: Message[], options: DialogueOption[]) => Promise<void>;
  onJumpToReplay?: (stepId: string) => void;
  height: number;
  onResizeStart: (e: React.MouseEvent) => void;
}> = ({
  step,
  isLeaf,
  isEffectivelyActive,
  onClose,
  onSave,
  onJumpToReplay,
  height,
  onResizeStart,
}) => {
  const [messages, setMessages] = useState<Message[]>(step.messages);
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [savedFlash, setSavedFlash] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset when step changes
  useEffect(() => {
    setMessages(step.messages);
    setEditingIdx(null);
    setError(null);
  }, [step.id]);

  const save = async () => {
    setIsSaving(true);
    setError(null);
    try {
      await onSave(step.id, messages, step.options);
      setSavedFlash(true);
      setTimeout(() => setSavedFlash(false), 1500);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSaving(false);
    }
  };

  const updateMsg = (idx: number, field: keyof Message, val: string) => {
    setMessages((prev) => prev.map((m, i) => (i === idx ? { ...m, [field]: val } : m)));
  };

  const typeColor: Record<string, string> = {
    YOU: "rgba(255,255,255,0.6)",
    CHARACTER: "#61afef",
    INNER_VOICE: "#c678dd",
    SYSTEM: "rgba(255,255,255,0.25)",
    ROLL: "#eab308",
    NOTIFICATION: "#98c379",
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
      {/* Resize handle at top edge */}
      <div
        onMouseDown={onResizeStart}
        className="absolute top-0 left-0 right-0 h-3 flex items-center justify-center cursor-ns-resize border-t border-white/10 group/rhandle z-10"
      >
        <div className="w-10 h-0.5 bg-white/15 rounded-full group-hover/rhandle:bg-white/40 transition-colors" />
      </div>

      {/* Content padded below handle */}
      <div className="flex flex-col h-full" style={{ paddingTop: "12px" }}>
        <div className="flex-shrink-0 flex items-center justify-between px-4 py-2 border-b border-white/6">
          <div className="flex items-center gap-3">
            <span className="text-[9px] font-mono text-white/30">{step.id}</span>
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
              {step.messages.length} msgs · {step.options.length} opts
            </span>
          </div>
          <div className="flex items-center gap-2">
            {onJumpToReplay && isEffectivelyActive && (
              <button
                onClick={() => onJumpToReplay(step.id)}
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
                      initialHeight={40}
                      minHeight={28}
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
                      style={{ color: typeColor[msg.type] ?? "rgba(255,255,255,0.4)" }}
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
            {step.options.map((opt) => (
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
            {step.options.length === 0 && (
              <p className="text-[9px] text-white/15 italic">No options</p>
            )}
          </div>
        </div>
      </div>
    </motion.div>
  );
};

// ── Main component ─────────────────────────────────────────────────────────

export const DialogueTreeGraph: React.FC<{
  onJumpToReplay?: (stepId: string) => void;
}> = ({ onJumpToReplay }) => {
  const [treeData, setTreeData] = useState<TreeData | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [layout, setLayout] = useState<NodePos[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pan, setPan] = useState({ x: 60, y: 60 });
  const [zoom, setZoom] = useState(1);
  const [inspectorHeight, setInspectorHeight] = useState(220);
  const containerRef = useRef<HTMLDivElement>(null);
  const isPanning = useRef(false);
  const lastMouse = useRef({ x: 0, y: 0 });
  const canvasSize = useRef({ w: 4000, h: 4000 });

  const fetchTree = useCallback(async () => {
    setIsLoading(true);
    try {
      const res = await fetch("/api/dialogue/tree");
      if (res.ok) {
        const data = await res.json();
        setTreeData(data);
        const positions = computeLayout(data.steps ?? {});
        setLayout(positions);
        // Auto-fit after load
        if (positions.length > 0) {
          requestAnimationFrame(() => fitToView(positions));
        }
      }
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchTree();
  }, [fetchTree]);

  const fitToView = useCallback(
    (positions?: NodePos[]) => {
      const pos = positions ?? layout;
      if (!pos.length || !containerRef.current) return;
      const pad = 60;
      const cw = containerRef.current.clientWidth;
      const ch = containerRef.current.clientHeight - (selectedId ? inspectorHeight : 0);
      const minX = Math.min(...pos.map((p) => p.x));
      const maxX = Math.max(...pos.map((p) => p.x + NODE_W));
      const minY = Math.min(...pos.map((p) => p.y));
      const maxY = Math.max(...pos.map((p) => p.y + NODE_H));
      const contentW = maxX - minX;
      const contentH = maxY - minY;
      const newZoom = Math.min((cw - pad * 2) / contentW, (ch - pad * 2) / contentH, 1.2);
      const scaledW = contentW * newZoom;
      const scaledH = contentH * newZoom;
      setPan({
        x: (cw - scaledW) / 2 - minX * newZoom,
        y: (ch - scaledH) / 2 - minY * newZoom,
      });
      setZoom(newZoom);
    },
    [layout, selectedId, inspectorHeight],
  );

  const resetZoom = () => {
    const rootPos = treeData?.stats.rootId
      ? layout.find((l) => l.id === treeData.stats.rootId)
      : null;
    const px = rootPos ? -rootPos.x + 60 : 60;
    const py = rootPos ? -rootPos.y + 60 : 60;
    setPan({ x: px, y: py });
    setZoom(1);
  };

  const handleInspectorResizeStart = (e: React.MouseEvent) => {
    const startY = e.clientY;
    const startH = inspectorHeight;
    e.preventDefault();
    const onMove = (ev: MouseEvent) => {
      const delta = ev.clientY - startY;
      setInspectorHeight(Math.max(120, Math.min(600, startH - delta)));
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  // Pan handlers
  const onMouseDown = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest("[data-node]")) return;
    isPanning.current = true;
    lastMouse.current = { x: e.clientX, y: e.clientY };
    e.currentTarget.setAttribute("style", "cursor:grabbing");
  };
  const onMouseMove = (e: React.MouseEvent) => {
    if (!isPanning.current) return;
    const dx = e.clientX - lastMouse.current.x;
    const dy = e.clientY - lastMouse.current.y;
    lastMouse.current = { x: e.clientX, y: e.clientY };
    setPan((p) => ({ x: p.x + dx, y: p.y + dy }));
  };
  const onMouseUp = (e: React.MouseEvent) => {
    isPanning.current = false;
    e.currentTarget.removeAttribute("style");
  };

  // Zoom handler
  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect) return;
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const factor = e.deltaY < 0 ? 1.1 : 0.9;
    const newZoom = Math.max(0.15, Math.min(3, zoom * factor));
    setPan((p) => ({
      x: mx - (mx - p.x) * (newZoom / zoom),
      y: my - (my - p.y) * (newZoom / zoom),
    }));
    setZoom(newZoom);
  };

  const handleNodeClick = (id: string) => {
    setSelectedId((prev) => (prev === id ? null : id));
  };

  const saveStep = async (id: string, messages: Message[], options: DialogueOption[]) => {
    const res = await fetch(`/api/dialogue/${id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ messages, options }),
    });
    if (!res.ok) throw new Error(await res.text());
    // Update local state
    setTreeData((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        steps: {
          ...prev.steps,
          [id]: { ...prev.steps[id], messages, options },
        },
      };
    });
  };

  const steps = treeData?.steps ?? {};
  const leafIds = new Set(treeData?.leafIds ?? []);
  const rootId = treeData?.stats.rootId ?? null;
  const selectedStep = selectedId ? steps[selectedId] : null;

  // A node is effectively active only if it AND all its ancestors are active.
  const effectivelyActiveIds = new Set<string>();
  for (const [id, step] of Object.entries(steps)) {
    let cur: StepData | undefined = step;
    let active = true;
    while (cur) {
      if (!cur.isActive) {
        active = false;
        break;
      }
      cur = cur.parentStepId ? steps[cur.parentStepId] : undefined;
    }
    if (active) effectivelyActiveIds.add(id);
  }

  // Compute bounding box for SVG
  let svgW = 2000;
  let svgH = 2000;
  if (layout.length > 0) {
    svgW = Math.max(...layout.map((p) => p.x + NODE_W)) + 200;
    svgH = Math.max(...layout.map((p) => p.y + NODE_H)) + 200;
  }

  const posMap = new Map(layout.map((p) => [p.id, p]));

  // Build edges
  const edges: Array<{
    key: string;
    sx: number;
    sy: number;
    ex: number;
    ey: number;
    label: string;
    isActive: boolean;
  }> = [];

  for (const [id, step] of Object.entries(steps)) {
    if (!step.parentStepId) continue;
    const parentPos = posMap.get(step.parentStepId);
    const childPos = posMap.get(id);
    if (!parentPos || !childPos) continue;
    const parentStep = steps[step.parentStepId];
    const option = parentStep?.options.find((o) => o.id === step.parentOptionId);
    const label = option ? option.text.slice(0, 22) + (option.text.length > 22 ? "…" : "") : "";
    edges.push({
      key: `${step.parentStepId}-${id}`,
      sx: parentPos.x + NODE_W / 2,
      sy: parentPos.y + NODE_H,
      ex: childPos.x + NODE_W / 2,
      ey: childPos.y,
      label,
      isActive: effectivelyActiveIds.has(id),
    });
  }

  const totalSteps = treeData?.stats.totalSteps ?? 0;
  const activeLeaves = treeData?.leafIds.length ?? 0;
  const branchCount = Object.values(steps).filter((s) => {
    return Object.values(steps).some((c) => c.parentStepId === s.id);
  }).length;

  return (
    <>
      <style>{`
        @keyframes leafPulse {
          0%, 100% { border-color: rgba(152,195,121,0.35); }
          50% { border-color: rgba(152,195,121,0.75); }
        }
        .leaf-pulse { animation: leafPulse 2s ease-in-out infinite; }
      `}</style>
      <div className="flex flex-col h-full overflow-hidden">
        {/* Toolbar */}
        <div className="flex items-center justify-between h-9 mb-1 flex-shrink-0">
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2 text-white/50">
              <GitBranch size={14} />
              <span className="text-[10px] font-bold uppercase tracking-[0.2em]">
                Dialogue_Tree
              </span>
            </div>
            <span className="text-white/20 text-[9px] font-mono">
              {totalSteps} steps · {branchCount} branches · {activeLeaves} leaves
            </span>
          </div>
          <div className="flex items-center gap-1.5">
            <button
              onClick={() => fitToView()}
              className="flex items-center gap-1.5 px-2.5 py-1 bg-white/5 text-white/40 hover:bg-white/8 hover:text-white rounded-sm border border-white/8 text-[9px] font-bold uppercase tracking-wider transition-all"
              title="Fit to view"
            >
              <Maximize2 size={11} />
              Fit
            </button>
            <button
              onClick={resetZoom}
              className="flex items-center gap-1.5 px-2.5 py-1 bg-white/5 text-white/40 hover:bg-white/8 hover:text-white rounded-sm border border-white/8 text-[9px] font-bold uppercase tracking-wider transition-all"
            >
              <RotateCcw size={11} />
              Reset
            </button>
            <button
              onClick={fetchTree}
              disabled={isLoading}
              className="flex items-center gap-1.5 px-2.5 py-1 bg-white/5 text-white/40 hover:bg-white/8 hover:text-white rounded-sm border border-white/8 text-[9px] font-bold uppercase tracking-wider transition-all disabled:opacity-40"
            >
              <RefreshCw size={11} className={isLoading ? "animate-spin" : ""} />
              Refresh
            </button>
          </div>
        </div>

        {/* Canvas */}
        <div
          ref={containerRef}
          className="flex-1 relative overflow-hidden bg-[#080809] border border-white/6 rounded-sm"
          style={{ cursor: "grab" }}
          onMouseDown={onMouseDown}
          onMouseMove={onMouseMove}
          onMouseUp={onMouseUp}
          onMouseLeave={onMouseUp}
          onWheel={onWheel}
        >
          {totalSteps === 0 && !isLoading && (
            <div className="absolute inset-0 flex flex-col items-center justify-center text-white/10 select-none">
              <GitBranch size={40} className="mb-4 opacity-30" />
              <p className="text-[9px] uppercase tracking-[0.35em] font-bold">No_Dialogue_Tree</p>
            </div>
          )}

          {/* Zoom indicator */}
          <div className="absolute bottom-3 right-3 text-[9px] font-mono text-white/15 pointer-events-none select-none">
            {Math.round(zoom * 100)}%
          </div>

          {/* Canvas inner */}
          <div
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: svgW,
              height: svgH,
              transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`,
              transformOrigin: "0 0",
            }}
          >
            {/* SVG edges */}
            <svg
              width={svgW}
              height={svgH}
              style={{ position: "absolute", top: 0, left: 0, pointerEvents: "none" }}
            >
              {edges.map((edge) => (
                <g key={edge.key}>
                  <path
                    d={edgePath(edge.sx, edge.sy, edge.ex, edge.ey)}
                    fill="none"
                    stroke={edge.isActive ? "rgba(255,255,255,0.12)" : "rgba(255,255,255,0.04)"}
                    strokeWidth={1}
                  />
                  {edge.label && (
                    <text
                      x={(edge.sx + edge.ex) / 2}
                      y={(edge.sy + edge.ey) / 2 - 4}
                      textAnchor="middle"
                      fill="rgba(255,255,255,0.18)"
                      fontSize={8}
                      fontFamily="monospace"
                    >
                      {edge.label}
                    </text>
                  )}
                </g>
              ))}
            </svg>

            {/* Nodes */}
            {layout.map(({ id, x, y }) => {
              const step = steps[id];
              if (!step) return null;
              return (
                <div key={id} data-node="1" onClick={() => handleNodeClick(id)}>
                  <GraphNode
                    step={step}
                    pos={{ id, x, y }}
                    isSelected={selectedId === id}
                    isLeaf={leafIds.has(id)}
                    isRoot={id === rootId}
                    isEffectivelyActive={effectivelyActiveIds.has(id)}
                    onClick={() => handleNodeClick(id)}
                  />
                </div>
              );
            })}
          </div>
        </div>

        {/* Inspector */}
        <AnimatePresence>
          {selectedStep && (
            <Inspector
              key={selectedStep.id}
              step={selectedStep}
              isLeaf={leafIds.has(selectedStep.id)}
              isEffectivelyActive={effectivelyActiveIds.has(selectedStep.id)}
              onClose={() => setSelectedId(null)}
              onSave={saveStep}
              onJumpToReplay={onJumpToReplay}
              height={inspectorHeight}
              onResizeStart={handleInspectorResizeStart}
            />
          )}
        </AnimatePresence>
      </div>
    </>
  );
};
