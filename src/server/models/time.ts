/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias 1289941679@qq.com
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
import { SEGMENT_LABELS, SEGMENT_HOURS } from "@/shared/constants";

// ── Types ──

export interface GameTime {
  day: number;
  segment: number;
}

interface TimePoint extends GameTime {
  id: string;
  label: string;
  _created_at: string;
}

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
    segment: Number(tp.segment),
    label: tp.label as string,
    _created_at: tp._created_at as string,
  };
}

// ── Advance Time ──

export async function advanceGameTime(
  segments: number,
): Promise<{ oldTime: GameTime; newTime: GameTime; totalSegments: number }> {
  const client = MemoryClient.getCachedInstance();
  const oldTime = await getCurrentTimePoint();
  const oldTimePoint = await getCurrentTimePoint();

  const totalSegments = oldTime.day * 12 + oldTime.segment + segments;
  const newDay = Math.floor(totalSegments / 12);
  const newSegment = totalSegments % 12;
  const label = describeSegment(newSegment);

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
         _id: $newId, day: $newDay, segment: $newSegment,
         label: $label, _created_at: datetime($now)
       })
       CREATE (old)-[r1:NEXT_TIMEPOINT]->(new)
       SET r1._created_at = datetime()
       DELETE r_del
       CREATE (a)-[r2:CURRENT_TIMEPOINT]->(new)
       SET r2._created_at = datetime()`,
      {
        oldId: oldTimePoint.id,
        newId,
        newDay,
        newSegment,
        label,
        now,
      },
    );
  } else {
    // First-ever TimePoint: no old tail to link
    await client.neo4j.executeWrite(
      `MATCH (a:TimeAnchor {_id: 'anchor'})
       CREATE (new:TimePoint {
         _id: $newId, day: $newDay, segment: $newSegment,
         label: $label, _created_at: datetime($now)
       })
       CREATE (a)-[r:CURRENT_TIMEPOINT]->(new)
       SET r._created_at = datetime()`,
      { newId, newDay, newSegment, label, now },
    );
  }

  const newTime: GameTime = { day: newDay, segment: newSegment };
  return { oldTime, newTime, totalSegments: segments };
}

// ── Helpers ──

function describeSegment(segment: number): string {
  return SEGMENT_LABELS[segment] ?? `Segment ${segment}`;
}

export function describeTime(time: GameTime): string {
  const label = SEGMENT_LABELS[time.segment] ?? `Segment ${time.segment}`;
  const hours = SEGMENT_HOURS[time.segment] ?? "";
  return `Day ${time.day}, ${label}${hours ? ` (~${hours})` : ""}`;
}
