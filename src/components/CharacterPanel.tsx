import React, { useState, useEffect } from "react";
import { motion, AnimatePresence } from "motion/react";
import { User, ChevronRight, MapPin, Box, Heart, Scroll, GitBranch } from "lucide-react";
import { useCharacter } from "@/context/CharacterContext";
import { CharacterStats } from "@/types/entities";
import type { Plot } from "@/types/plot";
import { worldManager } from "@/services/WorldManager";

const STATUS_STYLE: Record<string, { color: string; border: string }> = {
  PENDING: { color: "#61afef", border: "rgba(97,175,239,0.25)" },
  IN_PROGRESS: { color: "#eab308", border: "rgba(234,179,8,0.25)" },
  RESOLVED: { color: "#98c379", border: "rgba(152,195,121,0.25)" },
};

function isEffectivelyActive(plot: Plot, plots: Plot[]): boolean {
  const plotMap = new Map(plots.map((p) => [p.id, p]));
  let cur: Plot | undefined = plot;
  while (cur) {
    if (cur.status === "RESOLVED") return false;
    cur = cur.parentPlotId ? plotMap.get(cur.parentPlotId) : undefined;
  }
  return true;
}

function PlotNode({ plot, plots, depth = 0 }: { plot: Plot; plots: Plot[]; depth?: number }) {
  const style = STATUS_STYLE[plot.status] ?? STATUS_STYLE.PENDING;
  const active = isEffectivelyActive(plot, plots);
  const children = plots.filter((p) => p.parentPlotId === plot.id);

  return (
    <div className={depth > 0 ? "mt-2" : ""}>
      <div
        className={`p-3 bg-[#1a1a1a] border border-white/5 rounded-sm transition-opacity ${!active ? "opacity-40" : ""}`}
        style={
          depth > 0
            ? { marginLeft: `${depth * 12}px`, borderLeft: `2px solid rgba(255,107,53,0.2)` }
            : {}
        }
      >
        {depth > 0 && (
          <div className="flex items-center gap-1 text-[9px] text-gray-500 mb-1.5">
            <GitBranch size={9} />
            <span className="italic">
              {(() => {
                const parent = plots.find((p) => p.id === plot.parentPlotId);
                const opt = parent?.childPlots[plot.parentOptionId ?? -1];
                return opt ? `"${opt.triggerCondition}"` : "branch";
              })()}
            </span>
          </div>
        )}
        <div className="flex items-start justify-between gap-2 mb-1.5">
          <span className="text-[13px] font-sans text-white font-bold leading-snug">
            {plot.title}
          </span>
          <span
            className="text-[8px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded-sm border flex-shrink-0 mt-0.5"
            style={{ color: style.color, borderColor: style.border }}
          >
            {plot.status.replace("_", " ")}
          </span>
        </div>
        <p className="text-[11px] text-gray-400 line-clamp-2">{plot.description}</p>
        {plot.childPlots.length > 0 && (
          <div className="mt-2 pt-2 border-t border-white/5 space-y-1">
            {plot.childPlots.map((opt, i) => (
              <div key={i} className="flex items-start gap-1.5 text-[10px] text-gray-500">
                <span className="text-[#ff6b35]/50 mt-0.5">›</span>
                <span className={opt.plotId ? "text-gray-400" : "italic"}>
                  {opt.triggerCondition}
                  {opt.plotId && <span className="text-[#ff6b35]/70 ml-1">→ {opt.plotId}</span>}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
      {children.map((child) => (
        <PlotNode key={child.id} plot={child} plots={plots} depth={depth + 1} />
      ))}
    </div>
  );
}

function PlotTree({ plots }: { plots: Plot[] }) {
  const roots = plots.filter((p) => p.parentPlotId === null);
  return (
    <div className="space-y-3">
      {roots.map((root) => (
        <PlotNode key={root.id} plot={root} plots={plots} />
      ))}
    </div>
  );
}

export const CharacterPanel: React.FC = () => {
  const { character: liveCharacter } = useCharacter();
  const [isOpen, setIsOpen] = useState(false);
  const [activeTab, setActiveTab] = useState<"STATS" | "WORLD">("STATS");
  const [, forceUpdate] = useState(0);

  // Subscribe to worldManager so the panel re-renders on world/plot changes (live or replay)
  useEffect(() => worldManager.subscribe(() => forceUpdate((n) => n + 1)), []);

  const character = worldManager.getPlayerCharacter() ?? liveCharacter;
  const stats = Object.entries(character.stats) as [keyof CharacterStats, number][];
  const worldEntities = worldManager.getAllEntities();
  const plots = worldManager.getPlots();

  return (
    <>
      {/* Toggle Button */}
      <button
        id="character-panel-toggle"
        onClick={() => setIsOpen(!isOpen)}
        className="fixed top-8 right-8 z-50 h-11 min-w-[2.75rem] px-3 bg-[#1a1a1a] border border-[#ff6b35]/30 rounded-full text-[#ff6b35] hover:bg-[#ff6b35] hover:text-white transition-all duration-300 shadow-lg group flex items-center justify-center overflow-hidden"
      >
        <div className="flex items-center justify-center">
          <User size={20} className="shrink-0" />
          <span className="max-w-0 overflow-hidden group-hover:max-w-[120px] group-hover:ml-3 transition-all duration-300 ease-in-out whitespace-nowrap text-[12px] uppercase tracking-widest font-sans font-bold">
            Character
          </span>
        </div>
      </button>

      {/* Overlay */}
      <AnimatePresence>
        {isOpen && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={() => setIsOpen(false)}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm z-[51]"
          />
        )}
      </AnimatePresence>

      {/* Panel */}
      <AnimatePresence>
        {isOpen && (
          <motion.div
            initial={{ x: "100%" }}
            animate={{ x: 0 }}
            exit={{ x: "100%" }}
            transition={{ type: "spring", damping: 25, stiffness: 200 }}
            className="fixed top-0 right-0 h-full w-80 bg-[#0f0f0f] border-l border-[#ff6b35]/20 z-[52] shadow-2xl p-8 flex flex-col"
          >
            <div className="flex items-center justify-between mb-8">
              <h2 className="text-[20px] font-sans font-bold uppercase tracking-[0.2em] text-white">
                Notebook
              </h2>
              <button
                onClick={() => setIsOpen(false)}
                className="text-gray-500 hover:text-white transition-colors"
              >
                <ChevronRight size={24} />
              </button>
            </div>

            {/* Tabs */}
            <div className="flex border-b border-white/10 mb-8">
              <button
                onClick={() => setActiveTab("STATS")}
                className={`pb-2 px-4 text-[10px] uppercase tracking-[0.2em] transition-colors focus:outline-none ${activeTab === "STATS" ? "text-[#ff6b35] border-b-2 border-[#ff6b35]" : "text-gray-500 hover:text-gray-300"}`}
              >
                Attributes
              </button>
              <button
                onClick={() => setActiveTab("WORLD")}
                className={`pb-2 px-4 text-[10px] uppercase tracking-[0.2em] transition-colors focus:outline-none ${activeTab === "WORLD" ? "text-[#ff6b35] border-b-2 border-[#ff6b35]" : "text-gray-500 hover:text-gray-300"}`}
              >
                World
              </button>
            </div>

            <div className="flex-1 overflow-y-auto no-scrollbar space-y-8">
              {activeTab === "STATS" ? (
                <>
                  {/* Identity */}
                  <div>
                    <div className="text-[10px] uppercase tracking-[0.3em] text-[#ff6b35] mb-4">
                      Identity
                    </div>
                    <div className="p-4 bg-[#1a1a1a] border border-white/5 rounded-sm">
                      <div className="text-[18px] font-sans text-white mb-1">
                        {character.displayName}
                      </div>
                      <div className="text-[12px] text-gray-500 uppercase tracking-wider">
                        Wandering Outsider
                      </div>
                    </div>
                  </div>

                  {/* Stats */}
                  <div>
                    <div className="text-[10px] uppercase tracking-[0.3em] text-[#ff6b35] mb-4">
                      Attributes
                    </div>
                    <div className="space-y-3">
                      {stats.map(([name, value]) => (
                        <div key={name} className="group cursor-default">
                          <div className="flex justify-between items-end mb-1">
                            <span className="text-[12px] uppercase tracking-widest text-gray-400 group-hover:text-white transition-colors">
                              {name.replace(/_/g, " ")}
                            </span>
                            <span className="text-[14px] font-mono text-white font-bold">
                              {value}
                            </span>
                          </div>
                          <div className="h-[2px] bg-white/5 w-full relative overflow-hidden">
                            <motion.div
                              initial={{ width: 0 }}
                              animate={{ width: `${(value / 10) * 100}%` }}
                              className="absolute h-full bg-[#ff6b35]"
                            />
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                </>
              ) : (
                <>
                  {/* World Entities */}
                  <div>
                    <div className="text-[10px] uppercase tracking-[0.3em] text-[#ff6b35] mb-4">
                      Registered Entities
                    </div>
                    <div className="space-y-4">
                      {worldEntities.map((entity) => {
                        const Icon =
                          entity.type === "CHARACTER"
                            ? User
                            : entity.type === "LOCATION"
                              ? MapPin
                              : Box;
                        return (
                          <div
                            key={entity.id}
                            className="p-3 bg-[#1a1a1a] border border-white/5 rounded-sm"
                          >
                            <div className="flex items-center gap-2 mb-2">
                              <Icon size={14} className="text-gray-500" />
                              <span className="text-[14px] font-sans text-white font-bold">
                                {entity.displayName}
                              </span>
                            </div>
                            <p className="text-[11px] text-gray-400 line-clamp-2">
                              {entity.shortDescription}
                            </p>

                            {entity.type === "CHARACTER" && entity.opinions && (
                              <div className="mt-2 pt-2 border-t border-white/5">
                                <div className="flex items-center gap-1 text-[9px] text-[#ff6b35] uppercase tracking-widest mb-1">
                                  <Heart size={10} /> Opinions
                                </div>
                                {Object.entries(entity.opinions).map(([target, text]) => (
                                  <div key={target} className="text-[10px] text-gray-500 italic">
                                    <span className="text-gray-400 font-bold">{target}:</span> "
                                    {text}"
                                  </div>
                                ))}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  </div>

                  {/* Quests / Plots */}
                  <div>
                    <div className="text-[10px] uppercase tracking-[0.3em] text-[#ff6b35] mb-4 flex items-center gap-2">
                      <Scroll size={11} />
                      Quests
                    </div>
                    {plots.length === 0 ? (
                      <p className="text-[11px] text-gray-600 italic">No active quests.</p>
                    ) : (
                      <PlotTree plots={plots} />
                    )}
                  </div>
                </>
              )}
            </div>

            {/* Extra Info */}
            <div className="mt-8 pt-8 border-t border-white/5 opacity-50 text-[10px] uppercase tracking-widest leading-relaxed text-gray-500">
              You feel a strange sense of centralization. Your skills are now governed by a singular
              source of truth. The world responds to your inherent capabilities.
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
};
