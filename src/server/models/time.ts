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
  created_at: string;
}

// ── Current Time ──

export async function getGameTime(): Promise<GameTime> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(
    `MATCH (a:TimeAnchor {id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
     RETURN tp.day AS day, tp.segment AS segment`,
  );
  if (rows.length > 0) {
    return {
      day: Number(rows[0].day),
      segment: Number(rows[0].segment),
    };
  }
  // Fallback: old GameTime node (pre-migration)
  const legacy = await client.neo4j.executeRead(
    "MATCH (gt:GameTime {id: 'current'}) RETURN gt.day AS day, gt.segment AS segment",
  );
  if (legacy.length > 0) {
    return {
      day: Number(legacy[0].day),
      segment: Number(legacy[0].segment),
    };
  }
  return { day: 1, segment: 2 };
}

async function getCurrentTimePoint(): Promise<TimePoint | null> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(
    `MATCH (a:TimeAnchor {id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
     RETURN tp`,
  );
  if (rows.length === 0) return null;
  const tp = rows[0].tp as Record<string, unknown>;
  return {
    id: tp.id as string,
    day: Number(tp.day),
    segment: Number(tp.segment),
    label: tp.label as string,
    created_at: tp.created_at as string,
  };
}

// ── Advance Time ──

export async function advanceGameTime(
  segments: number,
): Promise<{ oldTime: GameTime; newTime: GameTime; totalSegments: number }> {
  const client = MemoryClient.getCachedInstance();
  const oldTime = await getGameTime();
  const oldTimePoint = await getCurrentTimePoint();

  const totalSegments = oldTime.day * 12 + oldTime.segment + segments;
  const newDay = Math.floor(totalSegments / 12);
  const newSegment = totalSegments % 12;
  const label = describeSegment(newSegment);

  const newId = uuidv4();
  const now = new Date().toISOString();

  // Ensure TimeAnchor exists
  await client.neo4j.executeWrite(`MERGE (a:TimeAnchor {id: 'anchor'})`);

  if (oldTimePoint) {
    await client.neo4j.executeWrite(
      `MATCH (a:TimeAnchor {id: 'anchor'})
       MATCH (old:TimePoint {id: $oldId})
       CREATE (new:TimePoint {
         id: $newId, day: $newDay, segment: $newSegment,
         label: $label, created_at: datetime($now)
       })
       CREATE (old)-[:NEXT_TIMEPOINT]->(new)
       DELETE (a)-[:CURRENT_TIMEPOINT]->(old)
       CREATE (a)-[:CURRENT_TIMEPOINT]->(new)`,
      { oldId: oldTimePoint.id, newId, newDay, newSegment, label, now },
    );
  } else {
    // First-ever TimePoint: no old tail to link
    await client.neo4j.executeWrite(
      `MATCH (a:TimeAnchor {id: 'anchor'})
       CREATE (new:TimePoint {
         id: $newId, day: $newDay, segment: $newSegment,
         label: $label, created_at: datetime($now)
       })
       CREATE (a)-[:CURRENT_TIMEPOINT]->(new)`,
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

// ── Migration ──

export async function migrateToTimePoints(
  defaultDay: number,
  defaultSegment: number,
): Promise<void> {
  const client = MemoryClient.getCachedInstance();

  const existing = await client.neo4j.executeRead(
    "MATCH (a:TimeAnchor {id: 'anchor'}) RETURN a LIMIT 1",
  );
  if (existing.length > 0) return;

  let day = defaultDay;
  let segment = defaultSegment;
  const legacy = await client.neo4j.executeRead(
    "MATCH (gt:GameTime {id: 'current'}) RETURN gt.day AS day, gt.segment AS segment",
  );
  if (legacy.length > 0) {
    day = Number(legacy[0].day);
    segment = Number(legacy[0].segment);
  }

  const label = describeSegment(segment);
  const now = new Date().toISOString();
  const id = uuidv4();

  await client.neo4j.executeWrite(
    `CREATE (a:TimeAnchor {id: 'anchor'})
     CREATE (tp:TimePoint {id: $id, day: $day, segment: $segment, label: $label, created_at: datetime($now)})
     CREATE (a)-[:CURRENT_TIMEPOINT]->(tp)`,
    { id, day, segment, label, now },
  );

  await client.neo4j.executeWrite("MATCH (gt:GameTime {id: 'current'}) DETACH DELETE gt");

  console.log(`[time] migrated to TimePoint: Day ${day}, Segment ${segment} (${label})`);
}
