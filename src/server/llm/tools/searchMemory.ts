import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { wrapSafe } from "@/server/llm/tools/shared";

export const searchMemory = tool({
  description: "Search world state (entities, messages) by meaning using vector similarity. Use when you need to find something not in the current scene.",
  inputSchema: z.object({
    query: z.string().describe("Natural language search query"),
    types: z.array(z.enum(["entities", "messages"])).default(["entities", "messages"]).describe("What to search"),
    limit: z.number().default(10).describe("Max results per type"),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();
    const results = await client.search.search(args.query, { memoryTypes: args.types, limit: args.limit });
    return JSON.stringify(results, null, 2);
  }, "searchMemory"),
});
