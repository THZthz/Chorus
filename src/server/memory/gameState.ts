import { MemoryClient } from "@/server/memory/client";
import type { DialogueOption } from "@/types/dialogue";

export const GAME_ID = "elysian-game";

/** Store current dialogue options on the one Conversation node for resume. */
export async function saveCurrentOptions(options: DialogueOption[]): Promise<void> {
  const client = MemoryClient.getCachedInstance();
  await client.neo4j.executeWrite(
    `MERGE (c:Conversation {session_id: $gameId})
     SET c.options = $options, c.updated_at = datetime()`,
    { gameId: GAME_ID, options: JSON.stringify(options) },
  );
}

/** Retrieve current dialogue options from the Conversation node. */
export async function getCurrentOptions(): Promise<{
  id: string;
  options: DialogueOption[];
} | null> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(
    `MATCH (c:Conversation {session_id: $gameId}) RETURN c.id AS id, c.options AS options`,
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
  return { id: row.id as string, options };
}
