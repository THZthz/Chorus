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

import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { CypherValidator } from "@/server/memory/validation";
import { stripHiddenProperties } from "@/server/memory/neo4j";
import { wrapSafe } from "@/server/llm/tools/shared";
import { resetEntityForQuery } from "@/server/llm/sceneObserver";
import { TOOL_NAMES } from "@/shared/constants";

const validator = new CypherValidator();
const AUTO_LIMIT = 50;

export const queryWorld = tool({
  title: TOOL_NAMES.QUERY_WORLD,
  description: `
READ or WRITE the world archive using Cypher.

READ — Look up facts. The current scene (player location, nearby NPCs, objects, inventory,
NPC dispositions, and active plots) is already pre-loaded under SCENE CONTEXT.
Do NOT re-query that information. Use READ for: entities at OTHER locations,
message history, timepoint history, relationship types, node type schemas,
or entity details not shown in the scene context.

WRITE — Persist changes. The archive IS the world — if you don't WRITE it, it didn't happen.
Every world mutation you narrate MUST be persisted. Use MERGE for upserts, SET for
property updates, DETACH DELETE for removal. Must include a WHERE clause when deleting.
Before creating nodes or relationships with new types, register them via \`${TOOL_NAMES.MANAGE_SCHEMA}\` first.
Never set a 'description' property directly on relationship instances in your Cypher.

Internal properties prefixed with "_" are hidden from READ results.
`.trim(),
  inputSchema: z.object({
    action: z
      .enum(["READ", "WRITE"])
      .default("READ")
      .describe("READ to query the world, WRITE to modify it."),
    query: z
      .string()
      .describe(
        "A Cypher query. READ: MATCH...RETURN. WRITE: CREATE, MERGE, SET, DELETE. Must include MATCH with WHERE for deletions.",
      ),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();

    if (args.action === "WRITE") {
      const validation = validator.validateWrite(args.query);
      if (!validation.valid) {
        return `VALIDATION FAILED:\n${validation.errors.join("; ")}.\nRewrite your query and retry.`;
      }

      try {
        try {
          await client.neo4j.executeRead(`EXPLAIN ${args.query}`);
        } catch (explainErr) {
          const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
          return `CYPHER SYNTAX ERROR:\n${msg}.\nFix your query and retry.`;
        }

        const rows = await client.neo4j.executeWrite(args.query);

        try {
          resetEntityForQuery(args.query);
        } catch {
          // Best-effort
        }

        return `Success. ${rows.length} row(s) affected.`;
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        return `QUERY ERROR:\n${msg}.\nAdjust your query and retry.`;
      }
    }

    // READ
    const validation = validator.validateRead(args.query);
    if (!validation.valid) {
      return `VALIDATION FAILED:\n${validation.errors.join("; ")}.\nRewrite your query and retry.`;
    }

    let query = args.query.trim();
    if (!/\bLIMIT\b/i.test(query)) {
      query = `${query} LIMIT ${AUTO_LIMIT}`;
    }

    try {
      try {
        await client.neo4j.executeRead(`EXPLAIN ${query}`);
      } catch (explainErr) {
        const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
        return `CYPHER SYNTAX ERROR:\n${msg}.\nFix your query and retry.`;
      }

      const rows = await client.neo4j.executeRead(query);
      const safeRows = stripHiddenProperties(rows);
      return JSON.stringify({ rowCount: safeRows.length, rows: safeRows }, null, 2);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return `QUERY ERROR:\n${msg}.\nAdjust your query and retry.`;
    }
  }, TOOL_NAMES.QUERY_WORLD),
});
