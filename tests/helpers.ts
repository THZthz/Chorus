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

import { MemoryClient } from "@/server/memory/client";
import { seedDatabase } from "@/server/stories/seed";
import { clearNeo4jDatabase } from "@/server/memory/reset";
import { RelationshipManager } from "@/server/relationshipManager";

export async function resetDb() {
  await clearNeo4jDatabase();

  // Reset in-memory GM_DEFINED types so stale registrations from previous
  // tests don't interfere with the fresh seed.
  const relManager = RelationshipManager.getCachedInstance();
  relManager.reset();
  const { NodeManager: NM } = await import("@/server/nodeManager");
  const nodeManager = NM.getCachedInstance();
  nodeManager.reset();

  await seedDatabase();

  // Sync INTERNAL + PREDEFINED types back to Neo4j after seed
  const client = MemoryClient.getCachedInstance();
  await relManager.syncToNeo4j(client.neo4j);
  await nodeManager.syncToNeo4j(client.neo4j);
}

export function resetObserver() {
  // SceneObserver removed — no-op.
}

export function parseToolOutput(output: string): Record<string, unknown> {
  try {
    return JSON.parse(output) as Record<string, unknown>;
  } catch {
    throw new Error(`Tool output is not valid JSON: ${output.slice(0, 200)}`);
  }
}

export async function isEmbedderAvailable(): Promise<boolean> {
  try {
    const client = MemoryClient.getCachedInstance();
    await client.search.searchByLabel("Entity", "test", { limit: 1 });
    return true;
  } catch {
    return false;
  }
}

export function createMockEventEmitter(): any {
  const events: Array<{ event: string; data: unknown }> = [];
  return {
    events,
    emitTimeUpdate(day: number, hour: number, hoursAdvanced: number) {
      events.push({ event: "time_update", data: { day, hour, hoursAdvanced } });
    },
    startStep: () => {},
    finish: () => {},
    emitStreamingReset: () => {},
    emitStreamingMessages: () => {},
    emitOptions: () => {},
    emitParsed: () => {},
    emitError: () => {},
    emitRollResult: () => {},
  };
}

/** Execute a tool's execute method, casting through any to bypass AI SDK types. */
export async function exec(tool: any, args: Record<string, unknown>): Promise<string> {
  return tool.execute(args) as unknown as string;
}
