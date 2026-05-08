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

import db from "@/server/db";
import type { GameTime, SceneState } from "@/types/entities";

// ── Segment Labels ──

export const SEGMENT_LABELS: Record<number, string> = {
  0: "Midnight",
  1: "Late Night",
  2: "Dawn",
  3: "Early Morning",
  4: "Morning",
  5: "Late Morning",
  6: "Noon",
  7: "Afternoon",
  8: "Late Afternoon",
  9: "Dusk",
  10: "Evening",
  11: "Night",
};

const SEGMENT_HOURS: Record<number, string> = {
  0: "12am–2am",
  1: "2am–4am",
  2: "4am–6am",
  3: "6am–8am",
  4: "8am–10am",
  5: "10am–12pm",
  6: "12pm–2pm",
  7: "2pm–4pm",
  8: "4pm–6pm",
  9: "6pm–8pm",
  10: "8pm–10pm",
  11: "10pm–12am",
};

// ── Time Functions ──

export function getGameTime(): GameTime {
  const dayRow = db.prepare("SELECT value FROM system_state WHERE key = ?").get("game_time_day") as
    | { value: string }
    | undefined;
  const segRow = db
    .prepare("SELECT value FROM system_state WHERE key = ?")
    .get("game_time_segment") as { value: string } | undefined;
  return {
    day: dayRow ? parseInt(dayRow.value, 10) : 1,
    segment: segRow ? parseInt(segRow.value, 10) : 0,
  };
}

export function setGameTime(time: GameTime): void {
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    "game_time_day",
    String(time.day),
  );
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    "game_time_segment",
    String(time.segment),
  );
}

export function advanceGameTime(segments: number): { oldTime: GameTime; newTime: GameTime } {
  const oldTime = getGameTime();
  const totalSegments = oldTime.day * 12 + oldTime.segment + segments;
  const newTime: GameTime = {
    day: Math.floor(totalSegments / 12),
    segment: totalSegments % 12,
  };
  setGameTime(newTime);
  return { oldTime, newTime };
}

export function describeTime(time: GameTime): string {
  const label = SEGMENT_LABELS[time.segment] ?? `Segment ${time.segment}`;
  const hours = SEGMENT_HOURS[time.segment] ?? "";
  return `Day ${time.day}, ${label}${hours ? ` (~${hours})` : ""}`;
}

// ── Scene Functions ──

const DEFAULT_SCENE: SceneState = {
  currentLocationId: "the_velvet_thorn",
  characterLocations: {
    veyla: "the_velvet_thorn",
    madam_cressida: "the_velvet_thorn",
  },
  objectPositions: {
    soul_shard: { type: "character", characterId: "player" },
    veyllas_ribbon: { type: "character", characterId: "player" },
  },
};

export function getSceneState(): SceneState {
  const row = db.prepare("SELECT value FROM system_state WHERE key = ?").get("current_scene") as
    | { value: string }
    | undefined;
  if (!row) return { ...DEFAULT_SCENE };
  try {
    return JSON.parse(row.value) as SceneState;
  } catch {
    return { ...DEFAULT_SCENE };
  }
}

export function setSceneState(scene: SceneState): void {
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    "current_scene",
    JSON.stringify(scene),
  );
}
