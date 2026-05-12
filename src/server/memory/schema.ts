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

import { Neo4jClient } from "@/server/memory/neo4j";

export async function setupSchema(
  client: Neo4jClient,
  vectorDimensions: number = 384,
): Promise<void> {
  // Unique constraints
  const constraints: [string, string, string][] = [
    ["conversation_id", "Conversation", "id"],
    ["message_id", "Message", "id"],
    ["entity_id", "Entity", "id"],
    ["note_id", "Note", "id"],
    ["plot_id", "Plot", "id"],
  ];

  for (const [name, label, prop] of constraints) {
    await client.executeWrite(
      `CREATE CONSTRAINT ${name} IF NOT EXISTS FOR (n:${label}) REQUIRE n.${prop} IS UNIQUE`,
    );
  }

  // Regular indexes
  const indexes: [string, string, string][] = [
    ["message_timestamp_idx", "Message", "timestamp"],
    ["entity_type_idx", "Entity", "type"],
    ["entity_name_idx", "Entity", "name"],
    ["plot_name_idx", "Plot", "name"],
    ["plot_status_idx", "Plot", "status"],
  ];

  for (const [name, label, prop] of indexes) {
    await client.executeWrite(`CREATE INDEX ${name} IF NOT EXISTS FOR (n:${label}) ON (n.${prop})`);
  }

  // NPCDisposition composite index
  try {
    await client.executeWrite(
      `CREATE INDEX npc_disposition_idx IF NOT EXISTS FOR (d:NPCDisposition) ON (d.npcName, d.targetName)`,
    );
  } catch {
    /* Neo4j version compat */
  }
  try {
    await client.executeWrite(
      `CREATE INDEX npc_disposition_target_idx IF NOT EXISTS FOR (d:NPCDisposition) ON (d.targetName)`,
    );
  } catch {
    /* Neo4j version compat */
  }

  // Vector indexes (require Neo4j 5.11+)
  const vectorIndexes: [string, string, string][] = [
    ["message_embedding_idx", "Message", "embedding"],
    ["entity_embedding_idx", "Entity", "embedding"],
    ["note_embedding_idx", "Note", "embedding"],
    ["plot_embedding_idx", "Plot", "embedding"],
  ];

  for (const [name, label, prop] of vectorIndexes) {
    try {
      await client.executeWrite(
        `CREATE VECTOR INDEX ${name} IF NOT EXISTS FOR (n:${label}) ON (n.${prop})
         OPTIONS { indexConfig: { \`vector.dimensions\`: ${vectorDimensions}, \`vector.similarity_function\`: 'COSINE' } }`,
      );
    } catch {
      console.warn(`[memory] Vector index ${name} not created (Neo4j 5.11+ required)`);
    }
  }
}
