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
