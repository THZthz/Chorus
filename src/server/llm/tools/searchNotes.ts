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
import { wrapSafe } from "@/server/llm/tools/shared";
import { TOOL_NAMES } from "@/shared/constants";

export const searchNotes = tool({
  title: TOOL_NAMES.SEARCH_NOTES,
  description: `
Search GM notes by meaning using vector similarity.
`.trim(),
  inputSchema: z.object({
    query: z
      .string()
      .describe(
        "Natural language search query, should be a list of keywords, keep short and focused.",
      ),
    limit: z.number().default(10).describe("Max results."),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();
    const notes = await client.notes.searchNotes(args.query, { limit: args.limit });
    const enriched = await Promise.all(
      notes.map(async (n) => ({
        name: n.name,
        content: n.content,
        similarity: n.similarity,
        aboutEntities: await client.notes.getLinkedEntities(n.name),
        aboutMessages: await client.notes.getLinkedMessages(n.name),
      })),
    );
    return JSON.stringify(enriched, null, 2);
  }, TOOL_NAMES.SEARCH_NOTES),
});
