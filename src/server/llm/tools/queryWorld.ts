import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { CypherValidator } from "@/server/memory/validation";
import { wrapSafe } from "@/server/llm/tools/shared";

const validator = new CypherValidator();
const AUTO_LIMIT = 50;

export const queryWorld = tool({
  description: `Read the game world using Cypher queries. Use to inspect entities, NPC dispositions, messages, and game time. The query MUST be read-only (MATCH, RETURN, ORDER BY, LIMIT). Use MATCH patterns to navigate relationships like LOCATED_AT, CARRIES, ALLIED_WITH, HOSTILE_TOWARDS. Entity types: PERSON, OBJECT, LOCATION, ORGANIZATION. GameTime node has day and segment properties.`,
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
      return JSON.stringify({ rowCount: rows.length, rows }, null, 2);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return `QUERY ERROR: ${msg}. Adjust your query and retry.`;
    }
  }, "queryWorld"),
});
