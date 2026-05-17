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

const SEARCH_TYPES = ["entities", "messages", "notes", "plots"] as const;

type SearchResult = {
  entities?: Record<string, unknown>[];
  messages?: Record<string, unknown>[];
  notes?: Record<string, unknown>[];
  plots?: Record<string, unknown>[];
};

export const searchWorld = tool({
  title: TOOL_NAMES.SEARCH_MEMORY,
  description: `
Search world state by meaning using vector similarity. Results are reranked by a reranker LLM model.
Use \`types\` to select which domains to search:
- \`entities\`: :Entity nodes, embedded by "name (Type): description"
- \`messages\`: Generated dialogue messages, embedded by full text
- \`notes\`: GM notes (:Note), embedded by full content
- \`plots\`: Story plots (:Plot), embedded by "name: description"
Omit \`types\` to search all domains.`.trim(),
  inputSchema: z.object({
    query: z
      .string()
      .describe(
        "Natural language search query, should be a list of keywords, keep short and focused.",
      ),
    types: z
      .array(z.enum(SEARCH_TYPES))
      .default(["entities", "messages", "notes", "plots"])
      .describe("What to search. Omit to search everything."),
    limit: z.number().default(10).describe("Max results per type."),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();
    const result: SearchResult = {};

    const tasks: Promise<void>[] = [];

    // Entities + messages: use the combined MemorySearch
    const worldTypes = args.types.filter((t) => t === "entities" || t === "messages");
    if (worldTypes.length > 0) {
      tasks.push(
        (async () => {
          const r = await client.search.search(args.query, {
            memoryTypes: worldTypes,
            limit: args.limit,
          });
          if (worldTypes.includes("entities")) {
            result.entities = stripHiddenProperties(r.entities.map((e) => ({ ...e }))) as Record<
              string,
              unknown
            >[];
          }
          if (worldTypes.includes("messages")) {
            result.messages = stripHiddenProperties(r.messages.map((m) => ({ ...m }))) as Record<
              string,
              unknown
            >[];
          }
        })(),
      );
    }

    // Notes
    if (args.types.includes("notes")) {
      tasks.push(
        (async () => {
          const notes = await client.notes.searchNotes(args.query, { limit: args.limit });
          result.notes = await Promise.all(
            notes.map(async (n) => ({
              name: n.name,
              content: n.content,
              similarity: n.similarity,
              aboutEntities: await client.notes.getLinkedEntities(n.name),
              aboutMessages: await client.notes.getLinkedMessages(n.name),
            })),
          );
        })(),
      );
    }

    // Plots
    if (args.types.includes("plots")) {
      tasks.push(
        (async () => {
          const plots = await client.plots.searchPlots(args.query, { limit: args.limit });
          result.plots = await Promise.all(
            plots.map(async (p) => {
              const children = await client.plots.getChildPlots(p.name);
              return {
                name: p.name,
                description: p.description,
                status: p.status,
                triggerCondition: p.triggerCondition,
                flags: p.flags,
                similarity: p.similarity,
                childPlots: children.map((c) => ({ name: c.name, status: c.status })),
              };
            }),
          );
        })(),
      );
    }

    // Run all selected searches in parallel
    await Promise.all(tasks);

    return JSON.stringify(result, null, 2);
  }, TOOL_NAMES.SEARCH_MEMORY),
});
