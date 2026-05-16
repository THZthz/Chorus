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
import { stripHiddenProperties } from "@/server/memory/neo4j";
import { wrapSafe } from "@/server/llm/tools/shared";
import { TOOL_NAMES } from "@/shared/constants";

export const searchWorld = tool({
  title: TOOL_NAMES.SEARCH_MEMORY,
  description: `
Search single world state (entities, messages) by meaning using vector similarity. The results will be reranked by a reranker LLM model.
Entities are node with :Entity. Messages are generated messages by tool \`${TOOL_NAMES.GENERATE_DIALOGUE}\`.
Entities are embedded by "name (Type): description". Messages are embedded with the full message text.
Use when you need to find something not in the current scene.`.trim(),
  inputSchema: z.object({
    query: z
      .string()
      .describe(
        "Natural language search query, should be a list of keywords, keep short and focused.",
      ),
    types: z
      .array(z.enum(["entities", "messages"]))
      .default(["entities", "messages"])
      .describe("What to search."),
    limit: z.number().default(10).describe("Max results per type."),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();
    const results = await client.search.search(args.query, {
      memoryTypes: args.types,
      limit: args.limit,
    });
    return JSON.stringify(stripHiddenProperties(results), null, 2);
  }, TOOL_NAMES.SEARCH_MEMORY),
});
