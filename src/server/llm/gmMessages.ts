/**
 * Chorus — cinematic RPG-style dialogue engine
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
    `MATCH (c:Conversation {session_id: $gameId})-[:_HAS_GM_MESSAGE]->(m:GMTurnMessage)
     RETURN m ORDER BY m.created_at, m.message_index`,
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

export async function saveGMMessages(messages: ModelMessage[], turnNumber: number): Promise<void> {
  const client = MemoryClient.getCachedInstance();
  const now = new Date().toISOString();

  const convRows = await client.neo4j.executeRead(
    `MATCH (c:Conversation {session_id: $gameId}) RETURN c._id AS id`,
    { gameId: GAME_ID },
  );
  if (convRows.length === 0) return;
  const convId = convRows[0].id as string;

  const toStore = messages.filter((m) => m.role !== "system");
  if (toStore.length === 0) return;

  const lastRows = await client.neo4j.executeRead(
    `MATCH (c:Conversation {_id: $convId})-[:_HAS_GM_MESSAGE]->(m:GMTurnMessage)
     WHERE NOT (m)-[:_NEXT_GM_MESSAGE]->(:GMTurnMessage)
     RETURN m._id AS id ORDER BY m.created_at DESC LIMIT 1`,
    { convId },
  );
  const previousLastId = lastRows.length > 0 ? (lastRows[0].id as string) : null;
  const isFirst = previousLastId === null;

  const ids: string[] = [];
  let messageIndex = 0;
  for (const msg of toStore) {
    const id = uuidv4();
    ids.push(id);
    await client.neo4j.executeWrite(
      `MATCH (c:Conversation {_id: $convId})
       CREATE (c)-[r:_HAS_GM_MESSAGE]->(m:GMTurnMessage {
         _id: $id,
         role: $role,
         content: $content,
         provider_options: $providerOptions,
         turn_number: $turnNumber,
         message_index: $messageIndex,
         created_at: datetime($now)
       })
       SET r.created_at = datetime()`,
      {
        convId,
        id,
        role: msg.role,
        content: JSON.stringify(msg.content),
        providerOptions: msg.providerOptions ? JSON.stringify(msg.providerOptions) : null,
        turnNumber,
        messageIndex,
        now,
      },
    );
    messageIndex++;
  }

  if (previousLastId) {
    await client.neo4j.createRelationship(
      "GMTurnMessage",
      "_id",
      previousLastId,
      "GMTurnMessage",
      "_id",
      ids[0],
      "_NEXT_GM_MESSAGE",
    );
  }
  for (let i = 0; i < ids.length - 1; i++) {
    await client.neo4j.createRelationship(
      "GMTurnMessage",
      "_id",
      ids[i],
      "GMTurnMessage",
      "_id",
      ids[i + 1],
      "_NEXT_GM_MESSAGE",
    );
  }
  if (isFirst && ids.length > 0) {
    await client.neo4j.createRelationship(
      "Conversation",
      "_id",
      convId,
      "GMTurnMessage",
      "_id",
      ids[0],
      "_FIRST_GM_MESSAGE",
    );
  }
}

export async function getNextTurnNumber(): Promise<number> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead(
    `MATCH (c:Conversation {session_id: $gameId})-[:_HAS_GM_MESSAGE]->(m:GMTurnMessage)
     RETURN max(m.turn_number) AS maxTurn`,
    { gameId: GAME_ID },
  );
  if (rows.length === 0 || rows[0].maxTurn === null) return 1;
  return (rows[0].maxTurn as number) + 1;
}
