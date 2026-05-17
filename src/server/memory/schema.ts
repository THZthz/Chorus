/**
 * Chorus — cinematic dialogue engine
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
    ["conversation__id", "Conversation", "_id"],
    ["message__id", "Message", "_id"],
    ["entity__id", "Entity", "_id"],
    ["note__id", "Note", "_id"],
    ["plot__id", "Plot", "_id"],
    ["timepoint__id", "TimePoint", "_id"],
    ["idcounter_key", "IdCounter", "session_id"],
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

  // TimePoint composite index
  await client.executeWrite(
    `CREATE INDEX timepoint_calendar_idx IF NOT EXISTS FOR (n:TimePoint) ON (n.day, n.segment)`,
  );

  // NPCDisposition composite index
  try {
    await client.executeWrite(
      `CREATE INDEX npc_disposition_idx IF NOT EXISTS FOR (d:NPCDisposition) ON (d.npc_name, d.target_name)`,
    );
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    console.warn(`[memory] npc_disposition_idx not created: ${msg}`);
  }
  try {
    await client.executeWrite(
      `CREATE INDEX npc_disposition_target_idx IF NOT EXISTS FOR (d:NPCDisposition) ON (d.target_name)`,
    );
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    console.warn(`[memory] npc_disposition_target_idx not created: ${msg}`);
  }

  // Vector indexes (require Neo4j 5.11+)
  const vectorIndexes: [string, string, string][] = [
    ["message_embedding_idx", "Message", "_embedding"],
    ["entity_embedding_idx", "Entity", "_embedding"],
    ["note_embedding_idx", "Note", "_embedding"],
    ["plot_embedding_idx", "Plot", "_embedding"],
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
