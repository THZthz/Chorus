import { MemoryClient } from "@/server/memory/client";
import type { DialogueOption } from "@/types/dialogue";

export const GAME_ID = "elysian-game";

export async function saveGameState(stepId: string, options: DialogueOption[]): Promise<void> {
  const client = MemoryClient.getCachedInstance();
  await client.neo4j.executeWrite(
    `MERGE (s:SessionState {id: $gameId})
     SET s.stepId = $stepId, s.options = $options, s.updated_at = datetime()`,
    { gameId: GAME_ID, stepId, options: JSON.stringify(options) },
  );
}

export async function getGameState(): Promise<{
  id: string;
  options: DialogueOption[];
} | null> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(
    `MATCH (s:SessionState {id: $gameId}) RETURN s.stepId AS stepId, s.options AS options`,
    { gameId: GAME_ID },
  );
  if (rows.length === 0) return null;
  const row = rows[0];
  let options: DialogueOption[] = [];
  try {
    options = JSON.parse(row.options as string);
  } catch {
    return null;
  }
  return { id: row.stepId as string, options };
}
