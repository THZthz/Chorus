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

import React, { useState, useEffect, useSyncExternalStore } from "react";
import { StickyNote, Eye, EyeOff, Link2, Clock, Map, AlertCircle } from "lucide-react";
import type { Fact } from "@/types/entities";
import { worldManager } from "@/services/WorldManager";

function resolveEntityName(id: string): string {
  const entity = worldManager.getEntity(id);
  return entity?.displayName ?? id;
}

// ── Live mode data ──────────────────────────────────────────────────────────

async function fetchLiveFacts(): Promise<Fact[]> {
  const res = await fetch("/api/facts");
  if (!res.ok) throw new Error(`Failed to fetch facts: ${res.status}`);
  return res.json();
}

// ── Replay snapshot access ──────────────────────────────────────────────────

function getReplayFacts(): Fact[] {
  return worldManager.getFacts();
}

function subscribeToReplay(cb: () => void) {
  return worldManager.subscribe(cb);
}

// ── Main component ──────────────────────────────────────────────────────────

export const FactsViewer: React.FC = () => {
  const isReplay = useSyncExternalStore(subscribeToReplay, () => worldManager.isReplayActive());

  const [liveFacts, setLiveFacts] = useState<Fact[] | null>(null);
  const [liveError, setLiveError] = useState<string | null>(null);
  const [filterEntityId, setFilterEntityId] = useState("");
  const [filterPlotId, setFilterPlotId] = useState("");
  const [showRemoved, setShowRemoved] = useState(false);

  useEffect(() => {
    if (isReplay) return;
    let cancelled = false;
    const load = async () => {
      try {
        const data = await fetchLiveFacts();
        if (!cancelled) {
          setLiveFacts(data);
          setLiveError(null);
        }
      } catch (e) {
        if (!cancelled) setLiveError(e instanceof Error ? e.message : String(e));
      }
    };
    load();
    return () => {
      cancelled = true;
    };
  }, [isReplay]);

  const facts = isReplay ? getReplayFacts() : (liveFacts ?? []);

  const filtered = facts.filter((f) => {
    if (!showRemoved && !f.isValid) return false;
    if (filterEntityId && !f.relatedEntityIds.some((id) => id.includes(filterEntityId)))
      return false;
    if (filterPlotId && !f.relatedPlotIds.some((id) => id.includes(filterPlotId))) return false;
    return true;
  });

  // ── Empty state ──────────────────────────────────────────────────────────

  if (!isReplay && !liveFacts && !liveError) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="flex flex-col items-center gap-3 text-white/20">
          <StickyNote size={24} />
          <span className="text-[9px] font-bold uppercase tracking-[0.2em]">
            Loading facts...
          </span>
        </div>
      </div>
    );
  }

  if (liveError) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="flex flex-col items-center gap-3 text-white/20">
          <AlertCircle size={24} className="text-red-400/40" />
          <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-red-400/40">
            {liveError}
          </span>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col min-h-0">
      {/* Filters */}
      <div className="flex items-center gap-3 mb-4 flex-shrink-0">
        <input
          type="text"
          placeholder="Filter by entity ID..."
          value={filterEntityId}
          onChange={(e) => setFilterEntityId(e.target.value)}
          className="px-2 py-1 text-[10px] bg-white/[0.03] border border-white/10 rounded-sm text-white/60 placeholder:text-white/15 font-mono focus:outline-none focus:border-white/20 w-40"
        />
        <input
          type="text"
          placeholder="Filter by plot ID..."
          value={filterPlotId}
          onChange={(e) => setFilterPlotId(e.target.value)}
          className="px-2 py-1 text-[10px] bg-white/[0.03] border border-white/10 rounded-sm text-white/60 placeholder:text-white/15 font-mono focus:outline-none focus:border-white/20 w-40"
        />
        <button
          onClick={() => setShowRemoved(!showRemoved)}
          className={`flex items-center gap-1.5 px-2 py-1 text-[10px] border rounded-sm transition-colors ${
            showRemoved
              ? "text-white border-white/20 bg-white/5"
              : "text-white/30 border-transparent hover:text-white/50"
          }`}
        >
          {showRemoved ? <Eye size={10} /> : <EyeOff size={10} />}
          {showRemoved ? "All" : "Valid"}
        </button>
        <span className="text-[9px] text-white/20 ml-auto font-mono">
          {filtered.length} fact{filtered.length !== 1 ? "s" : ""}
          {isReplay ? " (replay)" : ""}
        </span>
      </div>

      {/* Table */}
      <div className="flex-1 overflow-y-auto debug-scrollbar">
        {filtered.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full gap-3 text-white/15">
            <StickyNote size={24} />
            <span className="text-[9px] font-bold uppercase tracking-[0.2em]">
              No facts yet
            </span>
          </div>
        ) : (
          <table className="w-full text-left border-collapse">
            <thead>
              <tr className="border-b border-white/5">
                <th className="py-2 px-2 text-[9px] font-bold uppercase tracking-[0.15em] text-white/20 w-[180px]">
                  Key
                </th>
                <th className="py-2 px-2 text-[9px] font-bold uppercase tracking-[0.15em] text-white/20">
                  Value
                </th>
                <th className="py-2 px-2 text-[9px] font-bold uppercase tracking-[0.15em] text-white/20 w-[130px]">
                  Links
                </th>
                <th className="py-2 px-2 text-[9px] font-bold uppercase tracking-[0.15em] text-white/20 w-[16px]"></th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((fact) => (
                <tr
                  key={fact.id}
                  className={`border-b border-white/[0.02] hover:bg-white/[0.02] transition-colors ${
                    !fact.isValid ? "opacity-40" : ""
                  }`}
                >
                  <td className="py-2 px-2">
                    <span className="text-[10px] text-white/50 font-mono">{fact.key}</span>
                  </td>
                  <td className="py-2 px-2">
                    <span className="text-[11px] text-white/70">{fact.value}</span>
                  </td>
                  <td className="py-2 px-2">
                    <div className="flex flex-wrap items-center gap-1">
                      {fact.relatedEntityIds.map((id) => (
                        <span
                          key={id}
                          className="inline-flex items-center gap-0.5 px-1 py-0.5 text-[9px] bg-white/[0.04] border border-white/[0.06] rounded-sm text-white/35 font-mono"
                          title={resolveEntityName(id)}
                        >
                          <Link2 size={8} />
                          {id}
                        </span>
                      ))}
                      {fact.relatedPlotIds.map((id) => (
                        <span
                          key={id}
                          className="inline-flex items-center gap-0.5 px-1 py-0.5 text-[9px] bg-white/[0.04] border border-white/[0.06] rounded-sm text-white/35 font-mono"
                        >
                          <Link2 size={8} />
                          {id}
                        </span>
                      ))}
                      {fact.relatedScene && (
                        <span
                          className="inline-flex items-center gap-0.5 px-1 py-0.5 text-[9px] bg-white/[0.04] border border-white/[0.06] rounded-sm text-white/35"
                          title="Related to scene"
                        >
                          <Map size={8} />
                          scene
                        </span>
                      )}
                      {fact.relatedTime && (
                        <span
                          className="inline-flex items-center gap-0.5 px-1 py-0.5 text-[9px] bg-white/[0.04] border border-white/[0.06] rounded-sm text-white/35"
                          title="Related to time"
                        >
                          <Clock size={8} />
                          time
                        </span>
                      )}
                      {!fact.relatedEntityIds.length &&
                        !fact.relatedPlotIds.length &&
                        !fact.relatedScene &&
                        !fact.relatedTime && (
                          <span className="text-[9px] text-white/10 italic">none</span>
                        )}
                    </div>
                  </td>
                  <td className="py-2 px-2">
                    {!fact.isValid && (
                      <span className="text-[8px] text-red-400/30 font-bold uppercase">removed</span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
};
