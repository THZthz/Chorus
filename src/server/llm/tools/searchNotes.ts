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
import { wrapSafe } from "@/server/llm/tools/shared";

export const searchNotes = tool({
  description:
    "Search GM notes by meaning using vector similarity. Notes are your private scratchpad — use them to record thoughts, plans, and observations.",
  inputSchema: z.object({
    query: z.string().describe("Natural language search query"),
    limit: z.number().default(10).describe("Max results"),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();
    const notes = await client.notes.searchNotes(args.query, { limit: args.limit });
    const enriched = await Promise.all(
      notes.map(async (n) => ({
        id: n.id,
        content: n.content,
        similarity: n.similarity,
        aboutEntities: await client.notes.getLinkedEntities(n.id),
        aboutMessages: await client.notes.getLinkedMessages(n.id),
      })),
    );
    return JSON.stringify(enriched, null, 2);
  }, "searchNotes"),
});
