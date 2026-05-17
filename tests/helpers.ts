import { MemoryClient } from "@/server/memory/client";
import { seedDatabase } from "@/server/seed-stories/seed";
import { clearNeo4jDatabase } from "@/server/memory/reset";
import { getObserver } from "@/server/llm/sceneObserver";

export async function resetDb() {
  await clearNeo4jDatabase();
  await seedDatabase();
  getObserver().reset();
}

export function resetObserver() {
  getObserver().reset();
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
    await client.search.search("test", { memoryTypes: ["entities"], limit: 1 });
    return true;
  } catch {
    return false;
  }
}

export function createMockEventEmitter(): any {
  const events: Array<{ event: string; data: unknown }> = [];
  return {
    events,
    emitTimeUpdate(day: number, segment: number, segmentsAdvanced: number) {
      events.push({ event: "time_update", data: { day, segment, segmentsAdvanced } });
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
