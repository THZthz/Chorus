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
import { getEntityById, getEntitiesByIds, getEntitiesByText } from "@/server/models/world";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  id: z.string().optional().describe("Exact entity ID for single lookup (e.g. 'madam_vespera')."),
  ids: z
    .array(z.string())
    .optional()
    .describe(
      "Array of entity IDs for bulk lookup. Returns results in the same order, skipping missing IDs.",
    ),
  search: z
    .string()
    .optional()
    .describe("Text to search for in entity names/descriptions (up to 5 results)."),
});

export function createGetEntityTool() {
  return tool({
    title: "Get Entity",
    description:
      "Get full details of world entities. Provide an id for single lookup, ids array for bulk lookup, or a search term for text search (case-insensitive match on name/description, up to 5 results).",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>): Promise<string> => {
      if (args.id && args.ids && args.search) {
        return "ERROR: You can only search in only one of these ways: provide 'id' for single lookup, 'ids' for bulk lookup, or 'search' for text search.";
      }
      if (!args.id && !args.ids && !args.search) {
        return "ERROR: Search intention is unknown. Provide 'id' for single lookup, 'ids' for bulk lookup, or 'search' for text search.";
      }
      if (args.id) {
        const entity = getEntityById(args.id);
        if (!entity) {
          return `ERROR: Entity '${args.id}' not found. You may call ${TOOL_NAMES.LIST_ENTITIES}() to discover valid IDs.`;
        }
        return JSON.stringify(entity, null, 2);
      }
      if (args.ids && args.ids.length > 0) {
        const results = getEntitiesByIds(args.ids);
        if (results.length === 0) {
          return `ERROR: None of the requested IDs were found: [${args.ids.join(", ")}]. You may call ${TOOL_NAMES.LIST_ENTITIES}() to discover valid IDs.`;
        }
        if (results.length < args.ids.length) {
          const foundIds = new Set(results.map((e) => e.id));
          const missing = args.ids.filter((id) => !foundIds.has(id));
          return JSON.stringify(
            {
              note: `The following IDs were not found: [${missing.join(", ")}]. They may have been removed or misspelled.`,
              results,
            },
            null,
            2,
          );
        }
        return JSON.stringify(
          {
            note: "All IDs is successfully queried",
            results,
          },
          null,
          2,
        );
      }
      const results = getEntitiesByText(args.search!);
      if (results.length === 0) {
        return `No entities matched '${args.search}'. You may call ${TOOL_NAMES.LIST_ENTITIES}() to see all entities.`;
      }
      return JSON.stringify(results, null, 2);
    }, TOOL_NAMES.GET_ENTITY),
  });
}
