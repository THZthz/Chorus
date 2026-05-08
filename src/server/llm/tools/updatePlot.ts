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
import { updatePlot, getPlotById } from "@/server/models/plot";
import type { TurnEventEmitter } from "@/server/llm/events";
import { PLOT_STATUSES, type PlotOption } from "@/types/plot";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const plotOptionSchema = z.object({
  plotId: z.string().nullable().describe("ID of the child plot, or null if not created yet."),
  triggerCondition: z.string().describe("What player action activates this branch."),
});

const inputSchema = z.object({
  id: z.string().describe("The ID of the plot to update."),
  status: z.enum(PLOT_STATUSES).optional().describe("New status for the plot."),
  description: z.string().optional().describe("Updated plot description."),
  involvedLocations: z
    .array(z.string())
    .optional()
    .describe("Replacement list of involved location entity IDs."),
  involvedCharacters: z
    .array(z.string())
    .optional()
    .describe("Replacement list of involved character entity IDs."),
  childPlots: z
    .array(plotOptionSchema)
    .optional()
    .describe("Replacement list of branch options (replaces all existing childPlots)."),
  addChildPlot: plotOptionSchema
    .optional()
    .describe("Append a single branch option to the existing childPlots array."),
  removeChildPlot: z
    .number()
    .int()
    .min(0)
    .optional()
    .describe("Remove the branch option at this index from childPlots."),
  flags: z
    .record(z.string(), z.union([z.string(), z.boolean(), z.number()]))
    .optional()
    .describe("Key-value metadata scoped to this plot. Merged with existing flags."),
});

export function createUpdatePlotTool(events: TurnEventEmitter) {
  return tool({
    title: "Update Plot",
    description:
      "Update an existing plot's status, description, involved entities, or childPlots options. Only PENDING or IN_PROGRESS plots can be edited — RESOLVED plots are locked. Reports an error if the plot ID does not exist, the plot is RESOLVED, or the change would break the plot tree.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const result = updatePlot(args.id, {
        status: args.status,
        description: args.description,
        involvedLocations: args.involvedLocations,
        involvedCharacters: args.involvedCharacters,
        childPlots: args.childPlots as PlotOption[] | undefined,
        addChildPlot: args.addChildPlot as PlotOption | undefined,
        removeChildPlot: args.removeChildPlot,
        flags: args.flags,
      });

      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }

      const changes: Record<string, unknown> = {};
      if (args.status !== undefined) changes.status = args.status;
      if (args.description !== undefined) changes.description = args.description;
      if (args.involvedLocations !== undefined) changes.involvedLocations = args.involvedLocations;
      if (args.involvedCharacters !== undefined)
        changes.involvedCharacters = args.involvedCharacters;
      if (args.childPlots !== undefined) changes.childPlots = args.childPlots;
      if (args.addChildPlot !== undefined) changes.addChildPlot = args.addChildPlot;
      if (args.removeChildPlot !== undefined) changes.removeChildPlot = args.removeChildPlot;
      if (args.flags !== undefined) changes.flags = args.flags;
      events.emitPlotEdit(args.id, changes);

      const plot = getPlotById(args.id);
      if (!plot) return `Plot ${args.id} updated but could not be re-read.`;
      return `Plot "${plot.title}" (${args.id}) updated.`;
    }, TOOL_NAMES.UPDATE_PLOT),
  });
}
