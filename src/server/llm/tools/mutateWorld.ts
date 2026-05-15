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

import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { CypherValidator } from "@/server/memory/validation";
import { RelationshipManager } from "@/server/memory/relationshipManager";
import { wrapSafe } from "@/server/llm/tools/shared";
import { TOOL_NAMES } from "@/shared/constants";
import { resetEntityForQuery } from "@/server/llm/sceneObserver";

const validator = new CypherValidator();

export const mutateWorld = tool({
  title: TOOL_NAMES.MUTATE_WORLD,
  description: `
Modify the game world using Cypher queries.
Use to create/update/delete entities, move characters, set relationships, change NPC dispositions, and store player knowledge.
Use MERGE for upserts. Use SET to update properties. Use DETACH DELETE to remove entities. Must include a WHERE clause when deleting.
When creating a NEW relationship type (not LOCATED_AT, CARRIES, etc.), optionally provide 'description' to document what the type means — it will be stored as a :RelationshipType node for future reference.
Never set a 'description' property directly on relationship instances in your Cypher.
`.trim(),
  // NB: .nullable() on optional fields prevents Zod rejection when the LLM
  // outputs "field": null for fields it intends to omit.
  inputSchema: z.object({
    query: z
      .string()
      .describe(
        "A Cypher mutation query (CREATE, MERGE, SET, DELETE). Must include MATCH with WHERE for deletions.",
      ),
    description: z
      .string()
      .nullable()
      .optional()
      .describe(
        "When introducing a NEW relationship type, describe its meaning. Not needed for standard types like LOCATED_AT, CARRIES, HOSTILE_TOWARDS, ALLIED_WITH, LOCATED_IN.",
      ),
  }),
  execute: wrapSafe(async (args) => {
    const validation = validator.validateWrite(args.query);
    if (!validation.valid) {
      return `VALIDATION FAILED:\n${validation.errors.join("; ")}.\nRewrite your query and retry.`;
    }

    const client = MemoryClient.getCachedInstance();
    try {
      try {
        await client.neo4j.executeRead(`EXPLAIN ${args.query}`);
      } catch (explainErr) {
        const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
        return `CYPHER SYNTAX ERROR:\n${msg}.\nFix your query and retry.`;
      }

      const rows = await client.neo4j.executeWrite(args.query);

      // Sync relationship types to Neo4j. The validator auto-registers unknown types
      // with a placeholder description; if the GM provided a custom description, apply it.
      try {
        const manager = RelationshipManager.getCachedInstance();
        const types = validator.extractRelationshipTypes(args.query);
        if (args.description) {
          for (const type of types) {
            const existing = manager.get(type);
            if (!existing) {
              manager.register(type, args.description, "GM_DEFINED");
            } else {
              manager.updateDescription(type, args.description);
            }
          }
        }
        if (types.length > 0) {
          await manager.syncToNeo4j(client.neo4j);
        }
      } catch {
        // Relationship type sync is best-effort — never fail the tool for it
      }

      // Auto-reset observer for entities whose description/brief changed
      try {
        resetEntityForQuery(args.query);
      } catch {
        // Observer reset is best-effort — never fail the tool for it
      }

      return `Success. ${rows.length} row(s) affected.`;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return `QUERY ERROR:\n${msg}.\nAdjust your query and retry.`;
    }
  }, TOOL_NAMES.MUTATE_WORLD),
});
