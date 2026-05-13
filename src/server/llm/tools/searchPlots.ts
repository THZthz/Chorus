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
import {TOOL_NAMES} from "@/shared/constants";

export const searchPlots = tool({
  title: TOOL_NAMES.SEARCH_PLOTS,
  description:
    "Search plots by meaning using vector similarity. Use to find relevant story arcs, check plot status, and discover connected plots via BRANCHES_TO.",
  inputSchema: z.object({
    query: z.string().describe("Natural language search query"),
    limit: z.number().default(10).describe("Max results"),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();
    const plots = await client.plots.searchPlots(args.query, { limit: args.limit });
    const enriched = await Promise.all(
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
    return JSON.stringify(enriched, null, 2);
  }, TOOL_NAMES.SEARCH_PLOTS),
});
