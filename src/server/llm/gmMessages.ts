/**
 * Persisted GM conversation messages for multi-turn continuity.
 *
 * Stores AI SDK ModelMessage[] as :GMTurnMessage Neo4j nodes linked to
 * the Conversation node. These nodes are NOT visible to GM tools because
 * the CypherValidator allowlist (Entity, Message, NPCDisposition, GameTime,
 * TimePoint, TimeAnchor) does not include GMTurnMessage.
 */
import { v4 as uuidv4 } from "uuid";
import type { ModelMessage } from "ai";
import { MemoryClient } from "@/server/memory/client";
import { GAME_ID } from "@/server/memory/gameState";

export async function loadGMMessages(): Promise<ModelMessage[]> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(
    `MATCH (c:Conversation {session_id: $gameId})-[:HAS_GM_MESSAGE]->(m:GMTurnMessage)
     RETURN m ORDER BY m.created_at`,
    { gameId: GAME_ID },
  );

  return rows.map((r) => {
    const m = r.m as Record<string, unknown>;
    const msg: Record<string, unknown> = {
      role: m.role,
      content: JSON.parse(m.content as string),
    };
    if (m.provider_options) {
      msg.providerOptions = JSON.parse(m.provider_options as string);
    }
    return msg as unknown as ModelMessage;
  });
}

export async function saveGMMessages(
  messages: ModelMessage[],
  turnNumber: number,
): Promise<void> {
  const client = MemoryClient.getCachedInstance();
  const now = new Date().toISOString();

  const convRows = await client.neo4j.executeRead(
    `MATCH (c:Conversation {session_id: $gameId}) RETURN c.id AS id`,
    { gameId: GAME_ID },
  );
  if (convRows.length === 0) return;
  const convId = convRows[0].id as string;

  const toStore = messages.filter((m) => m.role !== "system");
  if (toStore.length === 0) return;

  const lastRows = await client.neo4j.executeRead(
    `MATCH (c:Conversation {id: $convId})-[:HAS_GM_MESSAGE]->(m:GMTurnMessage)
     WHERE NOT (m)-[:NEXT_GM_MESSAGE]->(:GMTurnMessage)
     RETURN m.id AS id ORDER BY m.created_at DESC LIMIT 1`,
    { convId },
  );
  const previousLastId = lastRows.length > 0 ? (lastRows[0].id as string) : null;
  const isFirst = previousLastId === null;

  const ids: string[] = [];
  for (const msg of toStore) {
    const id = uuidv4();
    ids.push(id);
    await client.neo4j.executeWrite(
      `MATCH (c:Conversation {id: $convId})
       CREATE (c)-[:HAS_GM_MESSAGE]->(m:GMTurnMessage {
         id: $id,
         role: $role,
         content: $content,
         provider_options: $providerOptions,
         turn_number: $turnNumber,
         created_at: datetime($now)
       })`,
      {
        convId,
        id,
        role: msg.role,
        content: JSON.stringify(msg.content),
        providerOptions: msg.providerOptions ? JSON.stringify(msg.providerOptions) : null,
        turnNumber,
        now,
      },
    );
  }

  if (previousLastId) {
    await client.neo4j.executeWrite(
      `MATCH (prev:GMTurnMessage {id: $prevId}), (next:GMTurnMessage {id: $nextId})
       CREATE (prev)-[:NEXT_GM_MESSAGE]->(next)`,
      { prevId: previousLastId, nextId: ids[0] },
    );
  }
  for (let i = 0; i < ids.length - 1; i++) {
    await client.neo4j.executeWrite(
      `MATCH (prev:GMTurnMessage {id: $prevId}), (next:GMTurnMessage {id: $nextId})
       CREATE (prev)-[:NEXT_GM_MESSAGE]->(next)`,
      { prevId: ids[i], nextId: ids[i + 1] },
    );
  }
  if (isFirst && ids.length > 0) {
    await client.neo4j.executeWrite(
      `MATCH (c:Conversation {id: $convId}), (m:GMTurnMessage {id: $msgId})
       CREATE (c)-[:FIRST_GM_MESSAGE]->(m)`,
      { convId, msgId: ids[0] },
    );
  }
}

export async function getNextTurnNumber(): Promise<number> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(
    `MATCH (c:Conversation {session_id: $gameId})-[:HAS_GM_MESSAGE]->(m:GMTurnMessage)
     RETURN max(m.turn_number) AS maxTurn`,
    { gameId: GAME_ID },
  );
  if (rows.length === 0 || rows[0].maxTurn === null) return 1;
  return (rows[0].maxTurn as number) + 1;
}
