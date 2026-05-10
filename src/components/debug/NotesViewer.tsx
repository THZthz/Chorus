import React, { useState, useEffect, useSyncExternalStore } from "react";
import { StickyNote, Eye, EyeOff, Link2, Clock, Map, AlertCircle } from "lucide-react";
import type { Note } from "@/types/entities";
import { worldManager } from "@/services/WorldManager";

function resolveEntityName(id: string): string {
  const entity = worldManager.getEntity(id);
  return entity?.displayName ?? id;
}

// ── Live mode data ──────────────────────────────────────────────────────────

async function fetchLiveNotes(): Promise<Note[]> {
  const res = await fetch("/api/notes");
  if (!res.ok) throw new Error(`Failed to fetch notes: ${res.status}`);
  return res.json();
}

// ── Replay snapshot access ──────────────────────────────────────────────────

function getReplayNotes(): Note[] {
  return worldManager.getNotes();
}

function subscribeToReplay(cb: () => void) {
  return worldManager.subscribe(cb);
}

// ── Main component ──────────────────────────────────────────────────────────

export const NotesViewer: React.FC = () => {
  const isReplay = useSyncExternalStore(subscribeToReplay, () => worldManager.isReplayActive());

  const [liveNotes, setLiveNotes] = useState<Note[] | null>(null);
  const [liveError, setLiveError] = useState<string | null>(null);
  const [filterEntityId, setFilterEntityId] = useState("");
  const [filterPlotId, setFilterPlotId] = useState("");
  const [showRemoved, setShowRemoved] = useState(false);

  useEffect(() => {
    if (isReplay) return;
    let cancelled = false;
    const load = async () => {
      try {
        const data = await fetchLiveNotes();
        if (!cancelled) {
          setLiveNotes(data);
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

  const notes = isReplay ? getReplayNotes() : (liveNotes ?? []);

  const filtered = notes.filter((n) => {
    if (!showRemoved && !n.isValid) return false;
    if (filterEntityId && !n.relatedEntityIds.some((id) => id.includes(filterEntityId)))
      return false;
    if (filterPlotId && !n.relatedPlotIds.some((id) => id.includes(filterPlotId))) return false;
    return true;
  });

  // ── Empty state ──────────────────────────────────────────────────────────

  if (!isReplay && !liveNotes && !liveError) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="flex flex-col items-center gap-3 text-white/20">
          <StickyNote size={24} />
          <span className="text-[9px] font-bold uppercase tracking-[0.2em]">Loading notes...</span>
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
          className="px-2 py-1 text-[10px] bg-white/[0.03] border border-white/10 rounded-sm text-white/60 placeholder:text-white/15 font-mono focus:outline-none focus:border-white/20 focus-visible:ring-1 focus-visible:ring-accent/50 w-40"
        />
        <input
          type="text"
          placeholder="Filter by plot ID..."
          value={filterPlotId}
          onChange={(e) => setFilterPlotId(e.target.value)}
          className="px-2 py-1 text-[10px] bg-white/[0.03] border border-white/10 rounded-sm text-white/60 placeholder:text-white/15 font-mono focus:outline-none focus:border-white/20 focus-visible:ring-1 focus-visible:ring-accent/50 w-40"
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
          {filtered.length} note{filtered.length !== 1 ? "s" : ""}
          {isReplay ? " (replay)" : ""}
        </span>
      </div>

      {/* Table */}
      <div className="flex-1 overflow-y-auto debug-scrollbar">
        {filtered.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full gap-3 text-white/15">
            <StickyNote size={24} />
            <span className="text-[9px] font-bold uppercase tracking-[0.2em]">No notes yet</span>
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
              {filtered.map((note) => (
                <tr
                  key={note.id}
                  className={`border-b border-white/[0.02] hover:bg-white/[0.02] transition-colors ${
                    !note.isValid ? "opacity-40" : ""
                  }`}
                >
                  <td className="py-2 px-2">
                    <span className="text-[10px] text-white/50 font-mono">{note.key}</span>
                  </td>
                  <td className="py-2 px-2">
                    <span className="text-[11px] text-white/70">{note.value}</span>
                  </td>
                  <td className="py-2 px-2">
                    <div className="flex flex-wrap items-center gap-1">
                      {note.relatedEntityIds.map((id) => (
                        <span
                          key={id}
                          className="inline-flex items-center gap-0.5 px-1 py-0.5 text-[9px] bg-white/[0.04] border border-white/[0.06] rounded-sm text-white/35 font-mono"
                          title={resolveEntityName(id)}
                        >
                          <Link2 size={8} />
                          {id}
                        </span>
                      ))}
                      {note.relatedPlotIds.map((id) => (
                        <span
                          key={id}
                          className="inline-flex items-center gap-0.5 px-1 py-0.5 text-[9px] bg-white/[0.04] border border-white/[0.06] rounded-sm text-white/35 font-mono"
                        >
                          <Link2 size={8} />
                          {id}
                        </span>
                      ))}
                      {note.relatedScene && (
                        <span
                          className="inline-flex items-center gap-0.5 px-1 py-0.5 text-[9px] bg-white/[0.04] border border-white/[0.06] rounded-sm text-white/35"
                          title="Related to scene"
                        >
                          <Map size={8} />
                          scene
                        </span>
                      )}
                      {note.relatedTime && (
                        <span
                          className="inline-flex items-center gap-0.5 px-1 py-0.5 text-[9px] bg-white/[0.04] border border-white/[0.06] rounded-sm text-white/35"
                          title="Related to time"
                        >
                          <Clock size={8} />
                          time
                        </span>
                      )}
                      {!note.relatedEntityIds.length &&
                        !note.relatedPlotIds.length &&
                        !note.relatedScene &&
                        !note.relatedTime && (
                          <span className="text-[9px] text-white/10 italic">none</span>
                        )}
                    </div>
                  </td>
                  <td className="py-2 px-2">
                    {!note.isValid && (
                      <span className="text-[8px] text-red-400/30 font-bold uppercase">
                        removed
                      </span>
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
