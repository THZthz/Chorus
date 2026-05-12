import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { CypherValidator } from "@/server/memory/validation";
import { wrapSafe } from "@/server/llm/tools/shared";

const validator = new CypherValidator();

export const mutateWorld = tool({
  description: `Modify the game world using Cypher queries. Use to create/update/delete entities, move characters, set relationships, change NPC dispositions, and store player knowledge. Allowed relationships: LOCATED_AT, CARRIES, ALLIED_WITH, HOSTILE_TOWARDS, LOCATED_IN, HAS_DISPOSITION. Use MERGE for upserts. Use SET to update properties. Use DETACH DELETE to remove entities. Must include a WHERE clause when deleting.`,
  inputSchema: z.object({
    query: z.string().describe("A Cypher mutation query (CREATE, MERGE, SET, DELETE). Must include MATCH with WHERE for deletions."),
  }),
  execute: wrapSafe(async (args) => {
    const validation = validator.validateWrite(args.query);
    if (!validation.valid) {
      return `VALIDATION FAILED: ${validation.errors.join("; ")}. Rewrite your query and retry.`;
    }

    const client = MemoryClient.getCachedInstance();
    try {
      try {
        await client.neo4j.executeRead(`EXPLAIN ${args.query}`);
      } catch (explainErr) {
        const msg = explainErr instanceof Error ? explainErr.message : String(explainErr);
        return `CYPHER SYNTAX ERROR: ${msg}. Fix your query and retry.`;
      }

      const rows = await client.neo4j.executeWrite(args.query);
      client.observer.onWorldChange({ action: "cypher", summary: args.query.slice(0, 200) });
      return JSON.stringify({ success: true, rowsAffected: rows.length });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return `QUERY ERROR: ${msg}. Adjust your query and retry.`;
    }
  }, "mutateWorld"),
});
