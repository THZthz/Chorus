import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { wrapSafe } from "@/server/llm/tools/shared";

export const searchNotes = tool({
  description: "Search GM notes by meaning using vector similarity. Notes are your private scratchpad — use them to record thoughts, plans, and observations.",
  inputSchema: z.object({
    query: z.string().describe("Natural language search query"),
    limit: z.number().default(10).describe("Max results"),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();
    const notes = await client.notes.searchNotes(args.query, { limit: args.limit });
    const enriched = await Promise.all(notes.map(async (n) => ({
      id: n.id, content: n.content, similarity: n.similarity,
      aboutEntities: await client.notes.getLinkedEntities(n.id),
      aboutMessages: await client.notes.getLinkedMessages(n.id),
    })));
    return JSON.stringify(enriched, null, 2);
  }, "searchNotes"),
});
