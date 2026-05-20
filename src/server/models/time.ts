/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias
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

import { v4 as uuidv4 } from "uuid";
import { MemoryClient } from "@/server/memory/client";

// ── Types ──

export interface GameTime {
  day: number;
  hour: number;
}

interface TimePoint extends GameTime {
  id: string;
  label: string;
  _created_at: string;
}

// ── Constants ──

const HALF_HOURS_PER_DAY = 48; // 24 hours × 2 half-hours

// ── Current Time ──

export async function getCurrentTimePoint(): Promise<TimePoint | null> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(
    `MATCH (a:TimeAnchor {_id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
     RETURN tp`,
  );
  if (rows.length === 0) return null;
  const tp = rows[0].tp as Record<string, unknown>;
  return {
    id: tp._id as string,
    day: Number(tp.day),
    hour: Number(tp.hour),
    label: tp.label as string,
    _created_at: tp._created_at as string,
  };
}

// ── Advance Time ──

export async function setInitialTime(day: number, hour: number): Promise<void> {
  const client = MemoryClient.getCachedInstance();
  const existing = await getCurrentTimePoint();
  if (existing) return; // already initialized

  const label = formatHour(hour);
  const newId = uuidv4();
  const now = new Date().toISOString();

  await client.neo4j.executeWrite(
    `MERGE (a:TimeAnchor {_id: 'anchor'})
     CREATE (new:TimePoint {
       _id: $newId, day: $day, hour: $hour,
       label: $label, _created_at: datetime($now)
     })
     CREATE (a)-[r:CURRENT_TIMEPOINT]->(new)
     SET r._created_at = datetime()`,
    { newId, day, hour, label, now },
  );
}

export async function advanceGameTime(
  halfHours: number,
  reason?: string | null,
): Promise<{ oldTime: GameTime; newTime: GameTime; totalHalfHours: number }> {
  const client = MemoryClient.getCachedInstance();
  const oldTimePoint = await getCurrentTimePoint();
  const oldTime: GameTime = oldTimePoint ?? { day: 1, hour: 8 };

  const oldTotal = oldTime.day * HALF_HOURS_PER_DAY + oldTime.hour * 2;
  const newTotal = oldTotal + halfHours;
  const newDay = Math.floor(newTotal / HALF_HOURS_PER_DAY);
  const newHour = (newTotal % HALF_HOURS_PER_DAY) / 2;
  const label = formatHour(newHour);

  const newId = uuidv4();
  const now = new Date().toISOString();

  // Ensure TimeAnchor exists
  await client.neo4j.executeWrite(`MERGE (a:TimeAnchor {_id: 'anchor'})`);

  if (oldTimePoint) {
    await client.neo4j.executeWrite(
      `MATCH (a:TimeAnchor {_id: 'anchor'})
       MATCH (old:TimePoint {_id: $oldId})
       MATCH (a)-[r_del:CURRENT_TIMEPOINT]->(old)
       CREATE (new:TimePoint {
         _id: $newId, day: $newDay, hour: $newHour,
         label: $label, _created_at: datetime($now)
       })
       CREATE (old)-[r1:NEXT_TIMEPOINT]->(new)
       SET r1._created_at = datetime()
       SET r1.reason = $reason
       DELETE r_del
       CREATE (a)-[r2:CURRENT_TIMEPOINT]->(new)
       SET r2._created_at = datetime()`,
      {
        oldId: oldTimePoint.id,
        newId,
        newDay,
        newHour,
        label,
        now,
        reason: reason ?? null,
      },
    );
  } else {
    // First-ever TimePoint: no old tail to link
    await client.neo4j.executeWrite(
      `MATCH (a:TimeAnchor {_id: 'anchor'})
       CREATE (new:TimePoint {
         _id: $newId, day: $newDay, hour: $newHour,
         label: $label, _created_at: datetime($now)
       })
       CREATE (a)-[r:CURRENT_TIMEPOINT]->(new)
       SET r._created_at = datetime()`,
      { newId, newDay, newHour, label, now },
    );
  }

  const newTime: GameTime = { day: newDay, hour: newHour };
  return { oldTime, newTime, totalHalfHours: halfHours };
}

// ── Helpers ──

/** Format a fractional hour (0–23.5) as a clock time string, e.g. 13.5 → "1:30 PM". */
function formatHour(hour: number): string {
  const h = Math.floor(hour);
  const m = hour % 1 === 0.5 ? 30 : 0;
  const period = h < 12 ? "AM" : "PM";
  const displayH = h === 0 ? 12 : h > 12 ? h - 12 : h;
  const mm = m === 0 ? "00" : "30";
  return `${displayH}:${mm} ${period}`;
}

export function describeTime(time: GameTime): string {
  return `Day ${time.day}, ${formatHour(time.hour)}`;
}
