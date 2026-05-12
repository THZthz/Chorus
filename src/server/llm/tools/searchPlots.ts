import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { wrapSafe } from "@/server/llm/tools/shared";

export const searchPlots = tool({
  description: "Search plots by meaning using vector similarity. Use to find relevant story arcs, check plot status, and discover connected plots via BRANCHES_TO.",
  inputSchema: z.object({
    query: z.string().describe("Natural language search query"),
    limit: z.number().default(10).describe("Max results"),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();
    const plots = await client.plots.searchPlots(args.query, { limit: args.limit });
    const enriched = await Promise.all(plots.map(async (p) => {
      const children = await client.plots.getChildPlots(p.name);
      return {
        name: p.name, description: p.description, status: p.status,
        triggerCondition: p.triggerCondition, flags: p.flags, similarity: p.similarity,
        childPlots: children.map((c) => ({ name: c.name, status: c.status })),
      };
    }));
    return JSON.stringify(enriched, null, 2);
  }, "searchPlots"),
});
