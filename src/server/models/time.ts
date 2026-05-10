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
import { SEGMENT_LABELS, SEGMENT_HOURS } from "@/shared/constants";

export interface GameTime {
  day: number;
  segment: number;
}

export function getGameTime(): GameTime {
  const dayRow = db.prepare("SELECT value FROM system_state WHERE key = ?").get("game_time_day") as { value: string } | undefined;
  const segRow = db.prepare("SELECT value FROM system_state WHERE key = ?").get("game_time_segment") as { value: string } | undefined;
  return {
    day: dayRow ? parseInt(dayRow.value, 10) : 1,
    segment: segRow ? parseInt(segRow.value, 10) : 2,
  };
}

export function setGameTime(time: GameTime): void {
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run("game_time_day", String(time.day));
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run("game_time_segment", String(time.segment));
}

export function advanceGameTime(segments: number): { oldTime: GameTime; newTime: GameTime; totalSegments: number } {
  const oldTime = getGameTime();
  const totalSegments = oldTime.day * 12 + oldTime.segment + segments;
  const newDay = Math.floor(totalSegments / 12);
  const newSegment = totalSegments % 12;
  const newTime: GameTime = { day: newDay, segment: newSegment };
  setGameTime(newTime);
  return { oldTime, newTime, totalSegments: segments };
}

export function describeTime(time: GameTime): string {
  const label = SEGMENT_LABELS[time.segment] ?? `Segment ${time.segment}`;
  const hours = SEGMENT_HOURS[time.segment] ?? "";
  return `Day ${time.day}, ${label}${hours ? ` (~${hours})` : ""}`;
}
