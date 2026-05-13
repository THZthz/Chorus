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

import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { CypherValidator } from "@/server/memory/validation";
import { stripHiddenProperties } from "@/server/memory/neo4j";
import { wrapSafe } from "@/server/llm/tools/shared";
import {TOOL_NAMES} from "@/shared/constants";

const validator = new CypherValidator();
const AUTO_LIMIT = 50;

export const queryWorld = tool({
  title: TOOL_NAMES.QUERY_WORLD,
  description: `
Read the game world using Cypher queries.
The query MUST be read-only (MATCH, RETURN, ORDER BY, LIMIT).
Use MATCH patterns to navigate relationships like LOCATED_AT, CARRIES, ALLIED_WITH, HOSTILE_TOWARDS.
Entity types: PERSON, OBJECT, LOCATION, ORGANIZATION. Current time: MATCH (a:TimeAnchor {id:'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint) RETURN tp.day, tp.segment, tp.label.
Browse time history via NEXT_TIMEPOINT.

NOTE:
The current scene (player location, nearby NPCs, objects, inventory, NPC dispositions, and active plots) is already pre-loaded in the user prompt under "SCENE CONTEXT".
Do NOT query for scene information that is already present.
Use queryWorld only for specific lookups BEYOND the pre-loaded context, such as: entity searches by name, message history, timepoint browsing, or finding entities/relationships not shown in the scene.`.trim(),
  inputSchema: z.object({
    query: z.string().describe("A read-only Cypher query (MATCH...RETURN)."),
  }),
  execute: wrapSafe(async (args) => {
    const validation = validator.validateRead(args.query);
    if (!validation.valid) {
      return `VALIDATION FAILED: ${validation.errors.join("; ")}. Rewrite your query and retry.`;
    }

    let query = args.query.trim();
    if (!/\bLIMIT\b/i.test(query)) {
      query = `${query} LIMIT ${AUTO_LIMIT}`;
    }

    const client = MemoryClient.getCachedInstance();
    try {
      try {
        await client.neo4j.executeRead(`EXPLAIN ${query}`);
      } catch (explainErr) {
        const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
        return `CYPHER SYNTAX ERROR: ${msg}. Fix your query and retry.`;
      }

      const rows = await client.neo4j.executeRead(query);
      const safeRows = stripHiddenProperties(rows);
      return JSON.stringify({ rowCount: safeRows.length, rows: safeRows }, null, 2);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return `QUERY ERROR: ${msg}. Adjust your query and retry.`;
    }
  }, TOOL_NAMES.QUERY_WORLD),
});
