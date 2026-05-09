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
import { Network, X, Save, AlertCircle } from "lucide-react";
import type { Plot } from "@/types/plot";
import { worldManager } from "@/services/WorldManager";
import { CustomSelect, ResizableTextarea } from "./shared";
import type { NodeGraphConfig, NodeRenderProps, InspectorProps } from "./NodeGraph";

// ═══════════════════════════════════════════════════════════════════════════
// Plot Tree Config
// ═══════════════════════════════════════════════════════════════════════════

const PLOT_NODE_W = 200;
const PLOT_NODE_H = 144;

const STATUS_COLORS: Record<string, string> = {
  PENDING: "#eab308",
  IN_PROGRESS: "#61afef",
  RESOLVED: "#98c379",
};

// ── Plot node card ────────────────────────────────────────────────────────

const PlotNodeCard: React.FC<NodeRenderProps<Plot>> = ({
  node,
  pos,
  isSelected,
  isLeaf,
  isRoot,
  isEffectivelyActive,
}) => {
  const preview = node.description
    ? node.description.slice(0, 42) + (node.description.length > 42 ? "…" : "")
    : "(no description)";
  const shortId = node.id.slice(0, 12) + (node.id.length > 12 ? "…" : "");
  const statusColor = STATUS_COLORS[node.status] ?? "rgba(255,255,255,0.2)";

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
        width: PLOT_NODE_W,
        height: PLOT_NODE_H,
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
          {node.title}
        </span>
        <span
          className="text-[7px] font-bold uppercase tracking-wider px-1 rounded-sm border flex-shrink-0"
          style={{
            color: statusColor,
            borderColor: `${statusColor}33`,
            opacity: isEffectivelyActive ? 1 : 0.5,
          }}
        >
          {(node.status ?? "PENDING").replace("_", " ")}
        </span>
      </div>
      {/* Description preview */}
      <div className="px-2.5 pt-1.5 flex-1 min-h-0">
        <p
          className="text-[10px] leading-snug italic line-clamp-2"
          style={{
            color: isEffectivelyActive ? "rgba(255,255,255,0.45)" : "rgba(255,255,255,0.2)",
          }}
        >
          {preview}
        </p>
      </div>
      {/* Flags */}
      {node.flags && Object.keys(node.flags).length > 0 && (
        <div className="px-2.5 py-0.5 flex items-center gap-1 flex-shrink-0 overflow-hidden">
          {Object.entries(node.flags)
            .slice(0, 3)
            .map(([k, v]) => (
              <span
                key={k}
                className="text-[7px] px-1 py-0.5 rounded-sm border max-w-[56px] truncate flex-shrink-0"
                style={{
                  color: isEffectivelyActive ? "rgba(255,255,255,0.35)" : "rgba(255,255,255,0.15)",
                  borderColor: isEffectivelyActive
                    ? "rgba(255,255,255,0.07)"
                    : "rgba(255,255,255,0.04)",
                  backgroundColor: isEffectivelyActive
                    ? "rgba(255,255,255,0.03)"
                    : "rgba(255,255,255,0.01)",
                }}
              >
                {typeof v === "boolean" ? k : `${k}:${String(v)}`}
              </span>
            ))}
          {Object.keys(node.flags).length > 3 && (
            <span className="text-[7px] text-white/15 flex-shrink-0">
              +{Object.keys(node.flags).length - 3}
            </span>
          )}
        </div>
      )}
      {/* Footer badges */}
      <div
        className="mt-auto px-2.5 py-1.5 flex items-center gap-2 border-t flex-shrink-0"
        style={{ borderColor: "rgba(255,255,255,0.1)" }}
      >
        <span className="text-[9px] font-mono text-white/20">
          {node.childPlots?.length ?? 0} branch{(node.childPlots?.length ?? 0) !== 1 ? "es" : ""}
        </span>
        {((node.involvedLocations?.length ?? 0) > 0 ||
          (node.involvedCharacters?.length ?? 0) > 0) && (
          <>
            <span className="text-white/10">·</span>
            <span className="text-[9px] font-mono text-white/20">
              {(node.involvedLocations?.length ?? 0) + (node.involvedCharacters?.length ?? 0)} refs
            </span>
          </>
        )}
      </div>
    </div>
  );
};

// ── Plot inspector ────────────────────────────────────────────────────────

const PlotInspector: React.FC<
  InspectorProps<Plot> & {
    isReplayActive: boolean;
    currentReplayStepId: string | null;
  }
> = ({
  node,
  isLeaf,
  isEffectivelyActive,
  onClose,
  onSaved,
  height,
  onResizeStart,
  isReplayActive,
  currentReplayStepId,
}) => {
  const [title, setTitle] = useState(node.title);
  const [description, setDescription] = useState(node.description);
  const [status, setStatus] = useState(node.status);
  const [locationsText, setLocationsText] = useState((node.involvedLocations ?? []).join(", "));
  const [charactersText, setCharactersText] = useState((node.involvedCharacters ?? []).join(", "));
  const [flags, setFlags] = useState<[string, string, string][]>(() =>
    Object.entries(node.flags ?? {}).map(
      ([k, v]) => [k, String(v), typeof v] as [string, string, string],
    ),
  );
  const [isSaving, setIsSaving] = useState(false);
  const [savedFlash, setSavedFlash] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setTitle(node.title);
    setDescription(node.description);
    setStatus(node.status);
    setLocationsText((node.involvedLocations ?? []).join(", "));
    setCharactersText((node.involvedCharacters ?? []).join(", "));
    setFlags(
      Object.entries(node.flags ?? {}).map(
        ([k, v]) => [k, String(v), typeof v] as [string, string, string],
      ),
    );
    setError(null);
  }, [node.id]);

  const save = async () => {
    setIsSaving(true);
    setError(null);
    try {
      const parsedFlags: Record<string, string | boolean | number> = {};
      for (const [k, v, t] of flags) {
        if (!k.trim()) continue;
        if (t === "boolean") parsedFlags[k.trim()] = v === "true";
        else if (t === "number") parsedFlags[k.trim()] = Number(v) || 0;
        else parsedFlags[k.trim()] = v;
      }

      const patch = {
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
        flags: parsedFlags,
      };

      if (isReplayActive && currentReplayStepId && worldManager.isReplayActive()) {
        const updated = worldManager.updatePlotInReplaySnapshot(node.id, patch);
        if (!updated) return;
        const snapshot = worldManager.getReplaySnapshot();
        if (!snapshot) return;
        const res = await fetch(`/api/dialogue/${currentReplayStepId}/snapshot`, {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ worldSnapshot: snapshot }),
        });
        if (!res.ok) throw new Error(await res.text());
      } else {
        const res = await fetch(`/api/plots/${node.id}`, {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(patch),
        });
        if (!res.ok) throw new Error(await res.text());
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
            <span className="text-[9px] font-mono text-white/30">{node.id}</span>
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
              {node.childPlots?.length ?? 0} branches
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

          <div>
            <label className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 block mb-1">
              Description
            </label>
            <ResizableTextarea
              className="w-full bg-white/[0.04] border border-white/8 rounded-sm px-2 py-1 text-[11px] font-mono text-white/70 focus:outline-none focus:border-white/15"
              initialHeight={96}
              minHeight={44}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>

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

          {/* Flags */}
          <div>
            <label className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 block mb-1">
              Flags
            </label>
            {flags.length === 0 ? (
              <p className="text-[10px] text-white/15 italic">No flags</p>
            ) : (
              <div className="space-y-1 mb-1.5">
                {flags.map(([k, v, t], i) => (
                  <div key={i} className="flex items-center gap-1.5">
                    <input
                      className="w-[36%] bg-white/[0.04] border border-white/8 rounded-sm px-1.5 py-0.5 text-[10px] font-mono text-white/60 focus:outline-none focus:border-white/15"
                      placeholder="key"
                      value={k}
                      onChange={(e) => {
                        const next = [...flags] as [string, string, string][];
                        next[i] = [e.target.value, v, t];
                        setFlags(next);
                      }}
                    />
                    <select
                      className="w-[13%] bg-white/[0.04] border border-white/8 rounded-sm px-0.5 py-0.5 text-[9px] font-mono text-white/50 focus:outline-none"
                      value={t}
                      onChange={(e) => {
                        const nextType = e.target.value;
                        const next = [...flags] as [string, string, string][];
                        const defaultVal =
                          nextType === "boolean" ? "true" : nextType === "number" ? "0" : "";
                        next[i] = [k, defaultVal, nextType];
                        setFlags(next);
                      }}
                    >
                      <option value="string">str</option>
                      <option value="boolean">bool</option>
                      <option value="number">num</option>
                    </select>
                    <input
                      className="flex-1 bg-white/[0.04] border border-white/8 rounded-sm px-1.5 py-0.5 text-[10px] font-mono text-white/60 focus:outline-none focus:border-white/15"
                      placeholder={t === "boolean" ? "true / false" : "value"}
                      value={v}
                      onChange={(e) => {
                        const next = [...flags] as [string, string, string][];
                        next[i] = [k, e.target.value, t];
                        setFlags(next);
                      }}
                    />
                    <button
                      onClick={() => {
                        const next = flags.filter((_, idx) => idx !== i);
                        setFlags(next);
                      }}
                      className="p-0.5 text-white/12 hover:text-red-400/60 transition-colors flex-shrink-0"
                    >
                      <X size={10} />
                    </button>
                  </div>
                ))}
              </div>
            )}
            <button
              onClick={() => setFlags([...flags, ["", "", "string"]])}
              className="text-[9px] text-white/25 hover:text-white/50 border border-dashed border-white/8 hover:border-white/15 rounded-sm px-2 py-0.5 transition-colors"
            >
              + add flag
            </button>
          </div>

          {/* Child Plots (read-only) */}
          <div>
            <label className="text-[8px] font-bold uppercase tracking-[0.2em] text-white/20 block mb-1">
              Child Plots
            </label>
            {!node.childPlots?.length ? (
              <p className="text-[10px] text-white/15 italic">No branch options</p>
            ) : (
              <div className="space-y-1">
                {node.childPlots.map((opt, i) => (
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

// ── Config factory ────────────────────────────────────────────────────────

export function createPlotConfig(opts: {
  isReplayActive: boolean;
  currentReplayStepId: string | null;
}): NodeGraphConfig<Plot> {
  const isReplayActiveRef = { current: opts.isReplayActive };

  return {
    nodeWidth: PLOT_NODE_W,
    nodeHeight: PLOT_NODE_H,
    title: "Plot_Tree",
    emptyLabel: "No_Plot_Tree",
    icon: <Network size={14} />,
    getParentId: (node) => node.parentPlotId,
    getSortKey: (node) => node.title ?? "",
    getStatsLabel: ({ total, branches, activeLeaves }) =>
      `${total} plots · ${branches} branches · ${activeLeaves} active leaves`,
    fetchData: async () => {
      isReplayActiveRef.current = opts.isReplayActive;
      if (opts.isReplayActive) {
        return [...worldManager.getPlots()];
      }
      const res = await fetch("/api/plots");
      if (!res.ok) return [];
      return await res.json();
    },
    subscribe: (onChange) => {
      return worldManager.subscribe(() => {
        if (isReplayActiveRef.current) {
          onChange();
        }
      });
    },
    isEffectivelyActive: (id, nodeMap) => {
      let cur: Plot | undefined = nodeMap[id] as Plot | undefined;
      while (cur) {
        if (cur.status === "RESOLVED") return false;
        cur = cur.parentPlotId ? (nodeMap[cur.parentPlotId] as Plot | undefined) : undefined;
      }
      return true;
    },
    isLeaf: (id, nodeMap) => {
      const plot = nodeMap[id] as Plot | undefined;
      if (!plot) return false;
      return !plot.childPlots?.some((opt) => opt.plotId !== null);
    },
    getRootId: (nodes) => {
      const root = nodes.find((n) => (n as Plot).parentPlotId === null);
      return root?.id ?? null;
    },
    getEdgeLabel: (childId, parentId, nodeMap) => {
      const parent = nodeMap[parentId] as Plot | undefined;
      const option = parent?.childPlots?.find((o) => o.plotId === childId);
      return option?.triggerCondition
        ? option.triggerCondition.slice(0, 22) + (option.triggerCondition.length > 22 ? "…" : "")
        : "";
    },
    renderNode: PlotNodeCard,
    renderInspector: (props) => (
      <PlotInspector
        {...props}
        isReplayActive={opts.isReplayActive}
        currentReplayStepId={opts.currentReplayStepId}
      />
    ),
  };
}
