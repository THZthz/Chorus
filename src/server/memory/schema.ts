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
    ["preference_id", "Preference", "id"],
    ["fact_id", "Fact", "id"],
    ["reasoning_trace_id", "ReasoningTrace", "id"],
    ["reasoning_step_id", "ReasoningStep", "id"],
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
    ["preference_category_idx", "Preference", "category"],
    ["trace_success_idx", "ReasoningTrace", "success"],
  ];

  for (const [name, label, prop] of indexes) {
    await client.executeWrite(`CREATE INDEX ${name} IF NOT EXISTS FOR (n:${label}) ON (n.${prop})`);
  }

  // Vector indexes (require Neo4j 5.11+)
  const vectorIndexes: [string, string, string][] = [
    ["message_embedding_idx", "Message", "embedding"],
    ["entity_embedding_idx", "Entity", "embedding"],
    ["preference_embedding_idx", "Preference", "embedding"],
    ["fact_embedding_idx", "Fact", "embedding"],
    ["task_embedding_idx", "ReasoningTrace", "task_embedding"],
    ["step_embedding_idx", "ReasoningStep", "embedding"],
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
