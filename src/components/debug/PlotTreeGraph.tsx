import React, { useState, useEffect, useRef, useCallback } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Maximize2, RotateCcw, RefreshCw, X, Save, AlertCircle, Network } from "lucide-react";
import type { Plot } from "@/types/plot";
import { worldManager } from "@/services/WorldManager";
import { CustomSelect, ResizableTextarea } from "./shared";

// ── Layout constants ───────────────────────────────────────────────────────

const NODE_W = 200;
const NODE_H = 130;
const H_GAP = 52;
const V_GAP = 96;

// ── Layout algorithm ────────────────────────────────────────────────────────

interface NodePos {
  id: string;
  x: number;
  y: number;
}

function computeLayout(plots: Record<string, Plot>): NodePos[] {
  const childrenMap = new Map<string | null, string[]>();
  for (const [id, plot] of Object.entries(plots)) {
    const parent = plot.parentPlotId ?? null;
    if (!childrenMap.has(parent)) childrenMap.set(parent, []);
    childrenMap.get(parent)!.push(id);
  }
  // Sort children by title for stable layout
  for (const [, children] of childrenMap) {
    children.sort((a, b) => (plots[a]?.title ?? "").localeCompare(plots[b]?.title ?? ""));
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

// ── Status helpers ─────────────────────────────────────────────────────────

const STATUS_COLORS: Record<string, string> = {
  PENDING: "#eab308",
  IN_PROGRESS: "#61afef",
  RESOLVED: "#98c379",
};

// ── Node component ─────────────────────────────────────────────────────────

const GraphNode: React.FC<{
  plot: Plot;
  pos: NodePos;
  isSelected: boolean;
  isLeaf: boolean;
  isRoot: boolean;
  isEffectivelyActive: boolean;
  onClick: () => void;
}> = ({ plot, pos, isSelected, isLeaf, isRoot, isEffectivelyActive }) => {
  const preview = plot.description
    ? plot.description.slice(0, 42) + (plot.description.length > 42 ? "…" : "")
    : "(no description)";
  const shortId = plot.id.slice(0, 12) + (plot.id.length > 12 ? "…" : "");
  const statusColor = STATUS_COLORS[plot.status] ?? "rgba(255,255,255,0.2)";

  let borderColor = "rgba(255,255,255,0.12)";
  let bgColor = "rgba(255,255,255,0.025)";
  if (!isEffectivelyActive) {
    borderColor = "rgba(255,255,255,0.05)";
    bgColor = "rgba(255,255,255,0.01)";
  } else if (isSelected) {
    borderColor = "rgba(255,255,255,0.55)";
    bgColor = "rgba(255,255,255,0.07)";
  } else if (isLeaf) {
    borderColor = `${statusColor}44`;
    bgColor = `${statusColor}08`;
  } else if (isRoot) {
    borderColor = `${statusColor}55`;
    bgColor = `${statusColor}06`;
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
        display: "flex",
        flexDirection: "column",
      }}
    >
      {/* Header */}
      <div
        className="px-2.5 py-1.5 flex items-center justify-between border-b flex-shrink-0"
        style={{ borderColor: "rgba(255,255,255,0.06)" }}
      >
        <span className="text-[9px] font-mono text-white/30 truncate">{shortId}</span>
        <div className="flex items-center gap-1 flex-shrink-0 ml-1">
          {isRoot && (
            <span className="text-[8px] font-bold uppercase tracking-wider text-white/40 border border-white/10 px-1 rounded-sm">
              ROOT
            </span>
          )}
          {!isEffectivelyActive && (
            <span className="text-[8px] font-bold uppercase tracking-wider text-white/20 border border-white/8 px-1 rounded-sm">
              DEAD
            </span>
          )}
          {isLeaf && isEffectivelyActive && (
            <span className="text-[8px] font-bold uppercase tracking-wider text-white/40 border border-white/10 px-1 rounded-sm">
              LEAF
            </span>
          )}
        </div>
      </div>
      {/* Title + status */}
      <div className="px-2.5 pt-1.5 flex items-center gap-1.5 flex-shrink-0">
        <span
          className="text-[10px] font-bold truncate"
          style={{
            color: isEffectivelyActive ? "rgba(255,255,255,0.8)" : "rgba(255,255,255,0.25)",
          }}
        >
          {plot.title}
        </span>
        <span
          className="text-[7px] font-bold uppercase tracking-wider px-1 rounded-sm border flex-shrink-0"
          style={{
            color: statusColor,
            borderColor: `${statusColor}33`,
            opacity: isEffectivelyActive ? 1 : 0.5,
          }}
        >
          {plot.status.replace("_", " ")}
        </span>
      </div>
      {/* Description preview */}
      <div className="px-2.5 pt-2 pb-4 flex-1 min-h-0">
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
        className="mt-auto px-2.5 py-1.5 flex items-center gap-2 border-t flex-shrink-0"
        style={{ borderColor: "rgba(255,255,255,0.1)" }}
      >
        <span className="text-[9px] font-mono text-white/20">
          {plot.childPlots.length} branch{plot.childPlots.length !== 1 ? "es" : ""}
        </span>
        {(plot.involvedLocations.length > 0 || plot.involvedCharacters.length > 0) && (
          <>
            <span className="text-white/10">·</span>
            <span className="text-[9px] font-mono text-white/20">
              {plot.involvedLocations.length + plot.involvedCharacters.length} refs
            </span>
          </>
        )}
      </div>
    </div>
  );
};

// ── Inspector panel ──────────────────────────────────────────────────────────

const Inspector: React.FC<{
  plot: Plot;
  isLeaf: boolean;
  isEffectivelyActive: boolean;
  onClose: () => void;
  onSave: (id: string, patch: Partial<Plot>) => Promise<void>;
  height: number;
  onResizeStart: (e: React.MouseEvent) => void;
}> = ({ plot, isLeaf, isEffectivelyActive, onClose, onSave, height, onResizeStart }) => {
  const [title, setTitle] = useState(plot.title);
  const [description, setDescription] = useState(plot.description);
  const [status, setStatus] = useState(plot.status);
  const [locationsText, setLocationsText] = useState(plot.involvedLocations.join(", "));
  const [charactersText, setCharactersText] = useState(plot.involvedCharacters.join(", "));
  const [isSaving, setIsSaving] = useState(false);
  const [savedFlash, setSavedFlash] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setTitle(plot.title);
    setDescription(plot.description);
    setStatus(plot.status);
    setLocationsText(plot.involvedLocations.join(", "));
    setCharactersText(plot.involvedCharacters.join(", "));
    setError(null);
  }, [plot.id]);

  const save = async () => {
    setIsSaving(true);
    setError(null);
    try {
      await onSave(plot.id, {
        title,
        description,
        status,
        involvedLocations: locationsText
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
        involvedCharacters: charactersText
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
      });
      setSavedFlash(true);
      setTimeout(() => setSavedFlash(false), 1500);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSaving(false);
    }
  };

  const statusColor = STATUS_COLORS[status] ?? "rgba(255,255,255,0.4)";

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
            <span className="text-[9px] font-mono text-white/30">{plot.id}</span>
            <span
              className="text-[8px] font-bold uppercase tracking-wider px-1 rounded-sm border"
              style={{ color: statusColor, borderColor: `${statusColor}33` }}
            >
              {status.replace("_", " ")}
            </span>
            {isLeaf && (
              <span className="text-[8px] font-bold uppercase tracking-wider text-white/30 border border-white/10 px-1 rounded-sm">
                LEAF
              </span>
            )}
            {!isEffectivelyActive && (
              <span className="text-[8px] font-bold uppercase tracking-wider text-white/20 border border-white/10 px-1 rounded-sm">
                INACTIVE
              </span>
            )}
            <span className="text-[9px] text-white/20 font-mono">
              {plot.childPlots.length} branches
            </span>
          </div>
          <div className="flex items-center gap-2">
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

        <div className="flex-1 overflow-y-auto debug-scrollbar p-3 space-y-3">
          {/* Title */}
          <div>
            <label className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 block mb-1">
              Title
            </label>
            <input
              className="w-full bg-white/[0.04] border border-white/8 rounded-sm px-2 py-1 text-[11px] font-mono text-white/70 focus:outline-none focus:border-white/15"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
          </div>

          {/* Status */}
          <div>
            <label className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 block mb-1">
              Status
            </label>
            <CustomSelect
              value={status}
              options={[
                { value: "PENDING", label: "PENDING" },
                { value: "IN_PROGRESS", label: "IN_PROGRESS" },
                { value: "RESOLVED", label: "RESOLVED" },
              ]}
              onChange={(v) => setStatus(v as Plot["status"])}
            />
          </div>

          {/* Description */}
          <div>
            <label className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 block mb-1">
              Description
            </label>
            <ResizableTextarea
              className="w-full bg-white/[0.04] border border-white/8 rounded-sm px-2 py-1 text-[11px] font-mono text-white/70 focus:outline-none focus:border-white/15"
              initialHeight={56}
              minHeight={40}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>

          {/* Involved Locations */}
          <div>
            <label className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 block mb-1">
              Involved Locations
            </label>
            <input
              className="w-full bg-white/[0.04] border border-white/8 rounded-sm px-2 py-1 text-[11px] font-mono text-white/70 focus:outline-none focus:border-white/15"
              placeholder="comma-separated IDs"
              value={locationsText}
              onChange={(e) => setLocationsText(e.target.value)}
            />
          </div>

          {/* Involved Characters */}
          <div>
            <label className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 block mb-1">
              Involved Characters
            </label>
            <input
              className="w-full bg-white/[0.04] border border-white/8 rounded-sm px-2 py-1 text-[11px] font-mono text-white/70 focus:outline-none focus:border-white/15"
              placeholder="comma-separated IDs"
              value={charactersText}
              onChange={(e) => setCharactersText(e.target.value)}
            />
          </div>

          {/* Child Plots (read-only) */}
          <div>
            <label className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 block mb-1">
              Child Plots
            </label>
            {plot.childPlots.length === 0 ? (
              <p className="text-[10px] text-white/15 italic">No branch options</p>
            ) : (
              <div className="space-y-1">
                {plot.childPlots.map((opt, i) => (
                  <div
                    key={i}
                    className="flex items-center gap-2 px-2 py-1.5 bg-white/[0.02] border border-white/6 rounded-sm"
                  >
                    <span className="text-[9px] font-mono text-white/20 flex-shrink-0">[{i}]</span>
                    <span className="text-[10px] text-white/50 flex-1 truncate">
                      {opt.triggerCondition || "(no condition)"}
                    </span>
                    {opt.plotId ? (
                      <span className="text-[9px] font-mono text-[#98c379]/50 flex-shrink-0">
                        → {opt.plotId.slice(0, 10)}…
                      </span>
                    ) : (
                      <span className="text-[9px] font-mono text-white/15 flex-shrink-0">
                        uncreated
                      </span>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </motion.div>
  );
};

// ── Main component ─────────────────────────────────────────────────────────

export const PlotTreeGraph: React.FC<{
  isReplayActive?: boolean;
  currentReplayStepId?: string | null;
}> = ({ isReplayActive = false, currentReplayStepId = null }) => {
  const [plots, setPlots] = useState<Plot[]>([]);
  const isReplayActiveRef = useRef(isReplayActive);
  isReplayActiveRef.current = isReplayActive;
  const [isLoading, setIsLoading] = useState(false);
  const [layout, setLayout] = useState<NodePos[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pan, setPan] = useState({ x: 60, y: 60 });
  const [zoom, setZoom] = useState(1);
  const [inspectorHeight, setInspectorHeight] = useState(220);
  const containerRef = useRef<HTMLDivElement>(null);
  const isPanning = useRef(false);
  const lastMouse = useRef({ x: 0, y: 0 });

  const fetchPlots = useCallback(async () => {
    setIsLoading(true);
    try {
      const res = await fetch("/api/plots");
      if (res.ok) {
        const data: Plot[] = await res.json();
        setPlots(data);
      }
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Subscribe to worldManager for replay snapshot sync
  useEffect(() => {
    const unsub = worldManager.subscribe(() => {
      if (isReplayActiveRef.current) {
        setPlots([...worldManager.getPlots()]);
      }
    });
    return unsub;
  }, []);

  // Load plots: from snapshot when replay is active, from API otherwise
  useEffect(() => {
    if (isReplayActive) {
      setPlots(worldManager.getPlots());
    } else {
      fetchPlots();
    }
  }, [isReplayActive, fetchPlots]);

  // Recompute layout when plots array changes (but not on every fetch callback identity change)
  useEffect(() => {
    const plotMap: Record<string, Plot> = {};
    for (const p of plots) plotMap[p.id] = p;
    const positions = computeLayout(plotMap);
    setLayout(positions);
    if (positions.length > 0) {
      requestAnimationFrame(() => fitToView(positions));
    }
  }, [plots]);

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
    const rootPlot = plots.find((p) => p.parentPlotId === null);
    const rootPos = rootPlot ? layout.find((l) => l.id === rootPlot.id) : null;
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

  const savePlot = async (id: string, patch: Partial<Plot>) => {
    // Replay mode with active snapshot: update snapshot (local + persisted to dialogue step)
    if (isReplayActive && currentReplayStepId && worldManager.isReplayActive()) {
      const updated = worldManager.updatePlotInReplaySnapshot(id, patch);
      if (!updated) return;
      const snapshot = worldManager.getReplaySnapshot();
      if (!snapshot) return;
      const res = await fetch(`/api/dialogue/${currentReplayStepId}/snapshot`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ worldSnapshot: snapshot }),
      });
      if (!res.ok) throw new Error(await res.text());
      return;
    }

    // Live mode (or replay with no snapshot): update the live plot in DB
    const res = await fetch(`/api/plots/${id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(patch),
    });
    if (!res.ok) throw new Error(await res.text());
    setPlots((prev) => prev.map((p) => (p.id === id ? { ...p, ...patch } : p)));
  };

  const plotMap: Record<string, Plot> = {};
  for (const p of plots) plotMap[p.id] = p;

  const rootId = plots.find((p) => p.parentPlotId === null)?.id ?? null;
  const selectedPlot = selectedId ? plotMap[selectedId] : null;

  // A node is effectively active only if it AND all ancestors are not RESOLVED
  const effectivelyActiveIds = new Set<string>();
  for (const [id, plot] of Object.entries(plotMap)) {
    let cur: Plot | undefined = plot;
    let active = true;
    while (cur) {
      if (cur.status === "RESOLVED") {
        active = false;
        break;
      }
      cur = cur.parentPlotId ? plotMap[cur.parentPlotId] : undefined;
    }
    if (active) effectivelyActiveIds.add(id);
  }

  // Leaves: plots with no childPlots that have non-null plotIds
  const leafIds = new Set<string>();
  for (const [id, plot] of Object.entries(plotMap)) {
    const hasConcreteChildren = plot.childPlots.some((opt) => opt.plotId !== null);
    if (!hasConcreteChildren) leafIds.add(id);
  }

  // Compute SVG bounds
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

  for (const [id, plot] of Object.entries(plotMap)) {
    if (!plot.parentPlotId) continue;
    const parentPos = posMap.get(plot.parentPlotId);
    const childPos = posMap.get(id);
    if (!parentPos || !childPos) continue;
    const parentPlot = plotMap[plot.parentPlotId];
    const option = parentPlot?.childPlots.find((o) => o.plotId === id);
    const label = option?.triggerCondition
      ? option.triggerCondition.slice(0, 22) + (option.triggerCondition.length > 22 ? "…" : "")
      : "";
    edges.push({
      key: `${plot.parentPlotId}-${id}`,
      sx: parentPos.x + NODE_W / 2,
      sy: parentPos.y + NODE_H,
      ex: childPos.x + NODE_W / 2,
      ey: childPos.y,
      label,
      isActive: effectivelyActiveIds.has(id),
    });
  }

  const totalPlots = plots.length;
  const branchCount = plots.filter((p) => plots.some((c) => c.parentPlotId === p.id)).length;
  const activeLeaves = Array.from(leafIds).filter((id) => effectivelyActiveIds.has(id)).length;

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Toolbar */}
      <div className="flex items-center justify-between h-9 mb-1 flex-shrink-0">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 text-white/50">
            <Network size={14} />
            <span className="text-[10px] font-bold uppercase tracking-[0.2em]">Plot_Tree</span>
          </div>
          <span className="text-white/20 text-[9px] font-mono">
            {totalPlots} plots · {branchCount} branches · {activeLeaves} active leaves
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
            onClick={() => {
              if (isReplayActive) {
                setPlots([...worldManager.getPlots()]);
              } else {
                fetchPlots();
              }
            }}
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
        {totalPlots === 0 && !isLoading && (
          <div className="absolute inset-0 flex flex-col items-center justify-center text-white/10 select-none">
            <Network size={40} className="mb-4 opacity-30" />
            <p className="text-[9px] uppercase tracking-[0.35em] font-bold">No_Plot_Tree</p>
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
            const plot = plotMap[id];
            if (!plot) return null;
            return (
              <div key={id} data-node="1" onClick={() => handleNodeClick(id)}>
                <GraphNode
                  plot={plot}
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
        {selectedPlot && (
          <Inspector
            key={selectedPlot.id}
            plot={selectedPlot}
            isLeaf={leafIds.has(selectedPlot.id)}
            isEffectivelyActive={effectivelyActiveIds.has(selectedPlot.id)}
            onClose={() => setSelectedId(null)}
            onSave={savePlot}
            height={inspectorHeight}
            onResizeStart={handleInspectorResizeStart}
          />
        )}
      </AnimatePresence>
    </div>
  );
};
