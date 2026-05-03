import React, { useState, useEffect, useRef, useCallback } from "react";
import { AnimatePresence } from "motion/react";
import { Maximize2, RotateCcw, RefreshCw } from "lucide-react";

// ── Types ──────────────────────────────────────────────────────────────────

export interface TreeNode {
  id: string;
}

export interface NodePos {
  id: string;
  x: number;
  y: number;
}

export interface NodeRenderProps<T extends TreeNode> {
  node: T;
  pos: NodePos;
  isSelected: boolean;
  isLeaf: boolean;
  isRoot: boolean;
  isEffectivelyActive: boolean;
}

export interface InspectorProps<T extends TreeNode> {
  node: T;
  isLeaf: boolean;
  isEffectivelyActive: boolean;
  onClose: () => void;
  onSaved: () => void;
  height: number;
  onResizeStart: (e: React.MouseEvent) => void;
}

export interface NodeGraphConfig<T extends TreeNode> {
  nodeWidth: number;
  nodeHeight: number;
  title: string;
  emptyLabel: string;
  icon: React.ReactNode;
  getParentId: (node: T) => string | null;
  getSortKey: (node: T) => string;
  getStatsLabel: (stats: { total: number; branches: number; activeLeaves: number }) => string;
  fetchData: () => Promise<T[]>;
  subscribe?: (onChange: () => void) => () => void;
  isEffectivelyActive: (id: string, nodeMap: Record<string, T>) => boolean;
  isLeaf: (id: string, nodeMap: Record<string, T>) => boolean;
  getRootId: (nodes: T[]) => string | null;
  getEdgeLabel: (childId: string, parentId: string, nodeMap: Record<string, T>) => string;
  renderNode: React.FC<NodeRenderProps<T>>;
  renderInspector?: React.FC<InspectorProps<T>>;
}

// ── Constants ──────────────────────────────────────────────────────────────

const H_GAP = 52;
const V_GAP = 96;

// ── SVG edge path ──────────────────────────────────────────────────────────

function edgePath(sx: number, sy: number, ex: number, ey: number): string {
  const my = (sy + ey) / 2;
  return `M ${sx} ${sy} C ${sx} ${my}, ${ex} ${my}, ${ex} ${ey}`;
}

// ── Layout algorithm ──────────────────────────────────────────────────────

function computeLayout<T extends TreeNode>(
  nodes: Record<string, T>,
  getParentId: (node: T) => string | null,
  getSortKey: (node: T) => string,
  NODE_W: number,
  NODE_H: number,
): NodePos[] {
  const childrenMap = new Map<string | null, string[]>();
  for (const [id, node] of Object.entries(nodes)) {
    const parent = getParentId(node) ?? null;
    if (!childrenMap.has(parent)) childrenMap.set(parent, []);
    childrenMap.get(parent)!.push(id);
  }
  for (const [, children] of childrenMap) {
    children.sort((a, b) =>
      (getSortKey(nodes[a]) ?? "").localeCompare(getSortKey(nodes[b]) ?? ""),
    );
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

// ── Main component ────────────────────────────────────────────────────────

export function NodeGraph<T extends TreeNode>({ config }: { config: NodeGraphConfig<T> }) {
  const configRef = useRef(config);
  configRef.current = config;

  const [nodes, setNodes] = useState<T[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [layout, setLayout] = useState<NodePos[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pan, setPan] = useState({ x: 60, y: 60 });
  const [zoom, setZoom] = useState(1);
  const [inspectorHeight, setInspectorHeight] = useState(220);
  const [isReady, setIsReady] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const isPanning = useRef(false);
  const lastMouse = useRef({ x: 0, y: 0 });
  const hasInitiallyFit = useRef(false);

  const cfg = configRef.current;
  const { NODE_W, NODE_H } = { NODE_W: cfg.nodeWidth, NODE_H: cfg.nodeHeight };

  // ── Data fetching ───────────────────────────────────────────────────────

  const loadData = useCallback(async () => {
    setIsLoading(true);
    try {
      const data = await configRef.current.fetchData();
      setNodes(data);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [config.fetchData, loadData]);

  // ── External subscription ───────────────────────────────────────────────

  useEffect(() => {
    const subscribe = configRef.current.subscribe;
    if (!subscribe) return;
    return subscribe(() => loadData());
  }, [config.subscribe, loadData]);

  // ── Fit-to-view ─────────────────────────────────────────────────────────

  const fitToView = useCallback(
    (positions?: NodePos[]) => {
      const cfg = configRef.current;
      const pos = positions ?? layout;
      if (!pos.length || !containerRef.current) return;
      const pad = 60;
      const cw = containerRef.current.clientWidth;
      const ch =
        containerRef.current.clientHeight -
        (selectedId && cfg.renderInspector ? inspectorHeight : 0);
      const minX = Math.min(...pos.map((p) => p.x));
      const maxX = Math.max(...pos.map((p) => p.x + cfg.nodeWidth));
      const minY = Math.min(...pos.map((p) => p.y));
      const maxY = Math.max(...pos.map((p) => p.y + cfg.nodeHeight));
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

  const fitToViewRef = useRef(fitToView);
  fitToViewRef.current = fitToView;

  // ── Layout recompute ────────────────────────────────────────────────────

  useEffect(() => {
    const cfg = configRef.current;
    const nodeMap: Record<string, T> = {};
    for (const n of nodes) nodeMap[n.id] = n;
    const positions = computeLayout(nodeMap, cfg.getParentId, cfg.getSortKey, cfg.nodeWidth, cfg.nodeHeight);
    setLayout(positions);
    if (positions.length > 0) {
      requestAnimationFrame(() => {
        fitToViewRef.current(positions);
        if (!hasInitiallyFit.current) {
          hasInitiallyFit.current = true;
          setIsReady(true);
        }
      });
    }
  }, [nodes]);

  // ── Reset zoom ──────────────────────────────────────────────────────────

  const resetZoom = useCallback(() => {
    const cfg = configRef.current;
    const rootId = cfg.getRootId(nodes);
    const rootPos = rootId ? layout.find((l) => l.id === rootId) : null;
    const px = rootPos ? -rootPos.x + 60 : 60;
    const py = rootPos ? -rootPos.y + 60 : 60;
    setPan({ x: px, y: py });
    setZoom(1);
  }, [nodes, layout]);

  // ── Inspector resize ────────────────────────────────────────────────────

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

  // ── Pan handlers ────────────────────────────────────────────────────────

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

  // ── Zoom handler ────────────────────────────────────────────────────────

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

  const handleSaved = useCallback(() => {
    loadData();
  }, [loadData]);

  // ── Derived data ────────────────────────────────────────────────────────

  const nodeMap: Record<string, T> = {};
  for (const n of nodes) nodeMap[n.id] = n;

  const rootId = cfg.getRootId(nodes);
  const selectedNode = selectedId ? nodeMap[selectedId] : null;

  const effectivelyActiveIds = new Set<string>();
  for (const id of Object.keys(nodeMap)) {
    if (cfg.isEffectivelyActive(id, nodeMap)) effectivelyActiveIds.add(id);
  }

  const leafIds = new Set<string>();
  for (const id of Object.keys(nodeMap)) {
    if (cfg.isLeaf(id, nodeMap)) leafIds.add(id);
  }

  // SVG bounds
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

  for (const [id] of Object.entries(nodeMap)) {
    const parentId = cfg.getParentId(nodeMap[id]);
    if (!parentId) continue;
    const parentPos = posMap.get(parentId);
    const childPos = posMap.get(id);
    if (!parentPos || !childPos) continue;
    const label = cfg.getEdgeLabel(id, parentId, nodeMap);
    edges.push({
      key: `${parentId}-${id}`,
      sx: parentPos.x + NODE_W / 2,
      sy: parentPos.y + NODE_H,
      ex: childPos.x + NODE_W / 2,
      ey: childPos.y,
      label,
      isActive: effectivelyActiveIds.has(id),
    });
  }

  const totalNodes = nodes.length;
  const branchCount = nodes.filter((n) =>
    nodes.some((c) => cfg.getParentId(c) === n.id),
  ).length;
  const activeLeaves = Array.from(leafIds).filter((id) => effectivelyActiveIds.has(id)).length;
  const statsLabel = cfg.getStatsLabel({ total: totalNodes, branches: branchCount, activeLeaves });

  const inspectorEnabled = !!cfg.renderInspector;

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
              {cfg.icon}
              <span className="text-[10px] font-bold uppercase tracking-[0.2em]">
                {cfg.title}
              </span>
            </div>
            <span className="text-white/20 text-[9px] font-mono">{statsLabel}</span>
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
              onClick={loadData}
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
          {totalNodes === 0 && !isLoading && (
            <div className="absolute inset-0 flex flex-col items-center justify-center text-white/10 select-none">
              <span className="mb-4 opacity-30">{cfg.icon}</span>
              <p className="text-[9px] uppercase tracking-[0.35em] font-bold">{cfg.emptyLabel}</p>
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
              opacity: isReady ? undefined : 0,
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
              const node = nodeMap[id];
              if (!node) return null;
              const NodeRenderer = cfg.renderNode;
              return (
                <div key={id} data-node="1" onClick={() => handleNodeClick(id)}>
                  <NodeRenderer
                    node={node}
                    pos={{ id, x, y }}
                    isSelected={selectedId === id}
                    isLeaf={leafIds.has(id)}
                    isRoot={id === rootId}
                    isEffectivelyActive={effectivelyActiveIds.has(id)}
                  />
                </div>
              );
            })}
          </div>
        </div>

        {/* Inspector */}
        {inspectorEnabled && cfg.renderInspector && (
          <AnimatePresence>
            {selectedNode && (
              <cfg.renderInspector
                key={selectedNode.id}
                node={selectedNode}
                isLeaf={leafIds.has(selectedNode.id)}
                isEffectivelyActive={effectivelyActiveIds.has(selectedNode.id)}
                onClose={() => setSelectedId(null)}
                onSaved={handleSaved}
                height={inspectorHeight}
                onResizeStart={handleInspectorResizeStart}
              />
            )}
          </AnimatePresence>
        )}
      </div>
    </>
  );
}
