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
import { addPlot } from "@/server/models/plot";
import { nextId } from "@/server/models/ids";
import type { TurnEventEmitter } from "@/server/llm/events";
import { PLOT_STATUSES, type PlotOption } from "@/types/plot";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const plotOptionSchema = z.object({
  plotId: z.string().nullable().describe("ID of the child plot, or null if not created yet."),
  triggerCondition: z.string().describe("What player action activates this branch."),
});

const inputSchema = z.object({
  title: z.string().describe("Concise title of the plot/quest."),
  description: z.string().describe("Detailed description of what this plot is about."),
  status: z.enum(PLOT_STATUSES).optional().describe("Initial status (default: PENDING)."),
  involvedLocations: z
    .array(z.string())
    .optional()
    .describe("Entity IDs of involved locations (prefer one)."),
  involvedCharacters: z
    .array(z.string())
    .optional()
    .describe("Entity IDs of involved characters (player is implicit)."),
  parentPlotId: z
    .string()
    .nullable()
    .optional()
    .describe("ID of the parent plot. Omit or set null for a root plot."),
  parentOptionId: z
    .number()
    .nullable()
    .optional()
    .describe("Index into parent.childPlots that this plot fulfils."),
  childPlots: z
    .array(plotOptionSchema)
    .optional()
    .describe("Pre-defined branch options for this plot."),
  flags: z
    .record(z.string(), z.union([z.string(), z.boolean(), z.number()]))
    .optional()
    .describe("Optional key-value metadata scoped to this plot (e.g. {'alarm_raised': true})."),
});

export function createCreatePlotTool(events: TurnEventEmitter) {
  return tool({
    title: "Create Plot",
    description:
      "Create a new plot node in the story tree. If this is the first plot, omit parentPlotId to create the root. Otherwise provide parentPlotId and parentOptionId (index into parent's childPlots array) to link it into the tree. The parent's childPlots[parentOptionId].plotId will be auto-updated.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const plotId = `plot_${nextId()}`;
      const result = addPlot({
        id: plotId,
        title: args.title,
        description: args.description,
        status: args.status ?? "PENDING",
        involvedLocations: args.involvedLocations ?? [],
        involvedCharacters: args.involvedCharacters ?? [],
        parentPlotId: args.parentPlotId ?? null,
        parentOptionId: args.parentOptionId ?? null,
        childPlots: (args.childPlots ?? []) as PlotOption[],
        flags: args.flags ?? {},
      });

      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }

      events.emitPlotCreate(plotId, args.title, args.parentPlotId ?? null);
      return `Plot created: "${args.title}" (${plotId}).`;
    }, TOOL_NAMES.CREATE_PLOT),
  });
}
