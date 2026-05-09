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
import { MapPin, Clock, User, Box, AlertCircle } from "lucide-react";
import type { GameTime, SceneState, WorldEntity } from "@/types/entities";
import { worldManager } from "@/services/WorldManager";
import { SEGMENT_LABELS, SEGMENT_HOURS } from "@/shared/constants";

function describeTime(time: GameTime): string {
  const label = SEGMENT_LABELS[time.segment] ?? `Segment ${time.segment}`;
  const hours = SEGMENT_HOURS[time.segment] ?? "";
  return `Day ${time.day}, ${label}${hours ? ` (~${hours})` : ""}`;
}

function resolveEntityName(id: string): string {
  const entity = worldManager.getEntity(id);
  return entity?.displayName ?? id;
}

function resolveEntityType(id: string): string | null {
  const entity = worldManager.getEntity(id);
  return entity?.type ?? null;
}

// ── Live mode data ──────────────────────────────────────────────────────────

interface LiveSceneData {
  gameTime: GameTime;
  scene: SceneState;
}

async function fetchLiveScene(): Promise<LiveSceneData> {
  const res = await fetch("/api/scene");
  if (!res.ok) throw new Error(`Failed to fetch scene: ${res.status}`);
  return res.json();
}

// ── Replay mode snapshot ────────────────────────────────────────────────────

function getReplaySnapshot() {
  return {
    gameTime: worldManager.getGameTime(),
    scene: worldManager.getScene(),
  };
}

function subscribeToReplay(cb: () => void) {
  return worldManager.subscribe(cb);
}

// ── Entity badge ────────────────────────────────────────────────────────────

const EntityBadge: React.FC<{ id: string; detail?: string }> = ({ id, detail }) => {
  const name = resolveEntityName(id);
  const type = resolveEntityType(id);
  return (
    <div className="flex items-center gap-1.5 px-2 py-1 bg-white/[0.03] border border-white/6 rounded-sm">
      {type === "CHARACTER" ? (
        <User size={10} className="text-white/30 flex-shrink-0" />
      ) : (
        <Box size={10} className="text-white/30 flex-shrink-0" />
      )}
      <span className="text-[10px] text-white/60 font-mono">{name}</span>
      {detail && <span className="text-[9px] text-white/20 ml-0.5">{detail}</span>}
    </div>
  );
};

// ── Main component ──────────────────────────────────────────────────────────

export const SceneViewer: React.FC = () => {
  const isReplay = useSyncExternalStore(subscribeToReplay, () => worldManager.isReplayActive());

  const [liveData, setLiveData] = useState<LiveSceneData | null>(null);
  const [liveError, setLiveError] = useState<string | null>(null);

  useEffect(() => {
    if (isReplay) return;
    let cancelled = false;
    const load = async () => {
      try {
        const data = await fetchLiveScene();
        if (!cancelled) {
          setLiveData(data);
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

  const gameTime: GameTime | null = isReplay
    ? worldManager.getGameTime()
    : (liveData?.gameTime ?? null);

  const scene: SceneState | null = isReplay ? worldManager.getScene() : (liveData?.scene ?? null);

  // ── Empty state ──────────────────────────────────────────────────────────

  if (!gameTime || !scene) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="flex flex-col items-center gap-3 text-white/20">
          {liveError ? (
            <>
              <AlertCircle size={24} className="text-red-400/40" />
              <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-red-400/40">
                {liveError}
              </span>
            </>
          ) : (
            <>
              <Clock size={24} />
              <span className="text-[9px] font-bold uppercase tracking-[0.2em]">
                No scene data yet
              </span>
            </>
          )}
        </div>
      </div>
    );
  }

  // ── Derived data ─────────────────────────────────────────────────────────

  const currentLocationId = scene.currentLocationId;
  const currentLocationName = resolveEntityName(currentLocationId);

  const charactersHere = Object.entries(scene.characterLocations)
    .filter(([, locId]) => locId === currentLocationId)
    .map(([charId]) => charId);

  const charactersElsewhere = Object.entries(scene.characterLocations).filter(
    ([, locId]) => locId !== currentLocationId,
  );

  const objectsAtLocation = Object.entries(scene.objectPositions)
    .filter(([, pos]) => pos.type === "location" && pos.locationId === currentLocationId)
    .map(([objId]) => objId);

  const objectsCarriedHere = Object.entries(scene.objectPositions)
    .filter(([, pos]) => pos.type === "character" && charactersHere.includes(pos.characterId))
    .map(([objId, pos]) => ({ objId, carrierId: (pos as { characterId: string }).characterId }));

  const objectsOther = Object.entries(scene.objectPositions).filter(([, pos]) => {
    if (pos.type === "location" && pos.locationId === currentLocationId) return false;
    if (pos.type === "character" && charactersHere.includes(pos.characterId)) return false;
    return true;
  });

  return (
    <div className="flex-1 overflow-y-auto debug-scrollbar space-y-5">
      {/* Time */}
      <section>
        <div className="flex items-center gap-2 mb-3">
          <Clock size={12} className="text-white/30" />
          <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-white/30">
            Time
          </span>
        </div>
        <p className="text-[13px] text-white/70 font-mono">{describeTime(gameTime)}</p>
      </section>

      {/* Current location */}
      <section>
        <div className="flex items-center gap-2 mb-3">
          <MapPin size={12} className="text-white/30" />
          <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-white/30">
            Current Location
          </span>
        </div>
        <EntityBadge id={currentLocationId} />
      </section>

      {/* Characters at location */}
      <section>
        <div className="flex items-center gap-2 mb-3">
          <User size={12} className="text-white/30" />
          <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-white/30">
            Characters Here
          </span>
        </div>
        {charactersHere.length === 0 ? (
          <p className="text-[10px] text-white/15 italic">No characters present</p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {charactersHere.map((id) => (
              <EntityBadge key={id} id={id} />
            ))}
          </div>
        )}
      </section>

      {/* Objects at location */}
      {objectsAtLocation.length > 0 && (
        <section>
          <div className="flex items-center gap-2 mb-3">
            <Box size={12} className="text-white/30" />
            <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-white/30">
              Objects at Location
            </span>
          </div>
          <div className="flex flex-wrap gap-2">
            {objectsAtLocation.map((id) => (
              <EntityBadge key={id} id={id} />
            ))}
          </div>
        </section>
      )}

      {/* Objects carried by characters here */}
      {objectsCarriedHere.length > 0 && (
        <section>
          <div className="flex items-center gap-2 mb-3">
            <Box size={12} className="text-white/30" />
            <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-white/30">
              Carried by Characters Here
            </span>
          </div>
          <div className="flex flex-wrap gap-2">
            {objectsCarriedHere.map(({ objId, carrierId }) => (
              <EntityBadge
                key={objId}
                id={objId}
                detail={`held by ${resolveEntityName(carrierId)}`}
              />
            ))}
          </div>
        </section>
      )}

      {/* Characters elsewhere */}
      {charactersElsewhere.length > 0 && (
        <section>
          <div className="flex items-center gap-2 mb-3">
            <User size={12} className="text-white/30" />
            <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-white/30">
              Characters Elsewhere
            </span>
          </div>
          <div className="space-y-1.5">
            {charactersElsewhere.map(([charId, locId]) => (
              <div key={charId} className="flex items-center gap-2 text-[10px]">
                <span className="text-white/50 font-mono">{resolveEntityName(charId)}</span>
                <span className="text-white/15">→</span>
                <span className="text-white/30 font-mono">{resolveEntityName(locId)}</span>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Other objects (elsewhere / carried elsewhere) */}
      {objectsOther.length > 0 && (
        <section>
          <div className="flex items-center gap-2 mb-3">
            <Box size={12} className="text-white/30" />
            <span className="text-[9px] font-bold uppercase tracking-[0.2em] text-white/30">
              Other Objects
            </span>
          </div>
          <div className="space-y-1.5">
            {objectsOther.map(([objId, pos]) => {
              const where =
                pos.type === "location"
                  ? `at ${resolveEntityName(pos.locationId)}`
                  : `with ${resolveEntityName(pos.characterId)}`;
              return (
                <div key={objId} className="flex items-center gap-2 text-[10px]">
                  <span className="text-white/50 font-mono">{resolveEntityName(objId)}</span>
                  <span className="text-white/20">{where}</span>
                </div>
              );
            })}
          </div>
        </section>
      )}
    </div>
  );
};
