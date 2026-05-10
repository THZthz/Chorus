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
import { getPlotById, getPlotsByIds, getAllPlots } from "@/server/models/plot";
import { PLOT_STATUSES } from "@/types/plot";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe, formatPlotMarkdown } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  id: z.string().optional().describe("Exact plot ID to fetch."),
  ids: z.array(z.string()).optional().describe("Array of plot IDs to bulk fetch."),
  status: z.enum(PLOT_STATUSES).optional().describe("Filter by status. Omit to return all plots."),
});

export function createGetPlotTool() {
  return tool({
    title: "Get Plot",
    description:
      "Retrieve plot(s): by single ID, by multiple IDs (bulk), or filter by status. Returns full plot data including childPlots.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      if (args.id && args.ids && args.ids.length !== 0) {
        return "ERROR: Provide either 'id' for a single plot or 'ids' for bulk fetch, not both.";
      }
      if (args.id) {
        const plot = getPlotById(args.id);
        if (!plot) {
          return `ERROR: Plot '${args.id}' not found. You may use ${TOOL_NAMES.GET_PLOT}() without an id to list all plots.`;
        }
        return formatPlotMarkdown(plot);
      }
      if (args.ids && args.ids.length > 0) {
        const plots = getPlotsByIds(args.ids);
        if (plots.length === 0) {
          return `No plots found with the provided IDs: [${args.ids.join(", ")}].`;
        }
        const found = new Set(plots.map((p) => p.id));
        const missing = args.ids.filter((id) => !found.has(id));
        const parts: string[] = [];
        if (missing.length > 0) {
          parts.push(`> Note: The following IDs were not found: [${missing.join(", ")}]`);
          parts.push("");
        }
        parts.push(`## Plots (${plots.length} results)`);
        parts.push("");
        for (const plot of plots) {
          parts.push(formatPlotMarkdown(plot));
          parts.push("");
          parts.push("---");
          parts.push("");
        }
        return parts.join("\n").trim();
      }
      const all = getAllPlots();
      const filtered = !args.status ? all : all.filter((p) => p.status === args.status);
      if (filtered.length === 0)
        return `No plots found${args.status ? ` with status ${args.status}` : ""}.`;
      const parts: string[] = [`## Plots (${filtered.length} total${args.status ? `, status: ${args.status}` : ""})`, ""];
      for (const plot of filtered) {
        parts.push(formatPlotMarkdown(plot));
        parts.push("");
        parts.push("---");
        parts.push("");
      }
      return parts.join("\n").trim();
    }, TOOL_NAMES.GET_PLOT),
  });
}
