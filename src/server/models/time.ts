import { MemoryClient } from "@/server/memory/client";
import { SEGMENT_LABELS, SEGMENT_HOURS } from "@/shared/constants";

export interface GameTime {
  day: number;
  segment: number;
}

export async function getGameTime(): Promise<GameTime> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(
    "MATCH (gt:GameTime {id: 'current'}) RETURN gt.day AS day, gt.segment AS segment",
  );
  if (rows.length === 0) {
    return { day: 1, segment: 2 };
  }
  const row = rows[0] as { day: number; segment: number };
  return {
    day: typeof row.day === "number" ? row.day : parseInt(String(row.day), 10),
    segment: typeof row.segment === "number" ? row.segment : parseInt(String(row.segment), 10),
  };
}

export async function setGameTime(time: GameTime): Promise<void> {
  const client = MemoryClient.getCachedInstance();
  await client.neo4j.executeWrite(
    `MERGE (gt:GameTime {id: 'current'})
     SET gt.day = $day, gt.segment = $segment`,
    { day: time.day, segment: time.segment },
  );
}

export async function advanceGameTime(segments: number): Promise<{ oldTime: GameTime; newTime: GameTime; totalSegments: number }> {
  const oldTime = await getGameTime();
  const totalSegments = oldTime.day * 12 + oldTime.segment + segments;
  const newDay = Math.floor(totalSegments / 12);
  const newSegment = totalSegments % 12;
  const newTime: GameTime = { day: newDay, segment: newSegment };
  await setGameTime(newTime);
  return { oldTime, newTime, totalSegments: segments };
}

export function describeTime(time: GameTime): string {
  const label = SEGMENT_LABELS[time.segment] ?? `Segment ${time.segment}`;
  const hours = SEGMENT_HOURS[time.segment] ?? "";
  return `Day ${time.day}, ${label}${hours ? ` (~${hours})` : ""}`;
}
