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
import { MemoryClient, PLOT_STATUSES } from "@/server/memory/client";
import { wrapSafe } from "@/server/llm/tools/shared";
import { TOOL_NAMES } from "@/shared/constants";

export const editPlot = tool({
  title: TOOL_NAMES.EDIT_PLOT,
  description: `Create, update, or delete a plot. Plots track story arcs. Use flags for player knowledge. Use branchTo/unbranch to connect child plots. Plots are separate from world entities — use ${TOOL_NAMES.SEARCH_PLOTS} to find them.`,
  inputSchema: z.object({
    plotName: z
      .string()
      .optional()
      .describe("Plot name. Omit to create, include to update/delete."),
    remove: z.boolean().default(false).describe("Set true to delete (requires plotName)."),
    description: z.string().optional().describe("Plot description."),
    status: z.enum(PLOT_STATUSES).optional().describe("Plot status."),
    triggerCondition: z.string().optional().describe("Condition that activates this plot."),
    setFlag: z
      .object({ flagId: z.string(), description: z.string() })
      .optional()
      .describe("Add or update a player flag on this plot."),
    removeFlag: z.string().optional().describe("Flag ID to remove."),
    branchTo: z.string().optional().describe("Child plot name to connect via BRANCHES_TO."),
    unbranch: z.string().optional().describe("Child plot name to disconnect."),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();

    if (args.plotName && args.remove) {
      const deleted = await client.plots.deletePlot(args.plotName);
      return JSON.stringify(deleted ? { removed: args.plotName } : { error: "Plot not found" });
    }

    if (!args.plotName) {
      if (!args.description) return JSON.stringify({ error: "description required for create" });
      const plot = await client.plots.createPlot(`plot_${Date.now()}`, {
        description: args.description,
        status: args.status ?? "PENDING",
        triggerCondition: args.triggerCondition,
      });
      return JSON.stringify({ created: plot.name, status: plot.status });
    }

    const existing = await client.plots.getPlot(args.plotName);
    if (!existing) return JSON.stringify({ error: `Plot "${args.plotName}" not found` });

    const oldStatus = existing.status;
    const newStatus = (args.status ?? oldStatus) as typeof oldStatus;

    const updates: Record<string, unknown> = {};
    if (args.description !== undefined) updates.description = args.description;
    if (args.status !== undefined) updates.status = args.status;
    if (args.triggerCondition !== undefined) updates.triggerCondition = args.triggerCondition;

    if (Object.keys(updates).length > 0) {
      await client.plots.updatePlot(args.plotName, updates as any);
    }

    // Auto-wire time relationships on status transition
    if (newStatus !== oldStatus) {
      if (oldStatus === "PENDING" && (newStatus === "ACTIVE" || newStatus === "IN_PROGRESS")) {
        await client.plots.markPlotStarted(args.plotName);
        await client.plots.markPlotActive(args.plotName);
      } else if (
        (newStatus === "ACTIVE" || newStatus === "IN_PROGRESS") &&
        oldStatus !== "ACTIVE" &&
        oldStatus !== "IN_PROGRESS"
      ) {
        await client.plots.markPlotActive(args.plotName);
      } else if (newStatus === "COMPLETED") {
        await client.plots.markPlotCompleted(args.plotName);
      }
    }

    if (args.setFlag) {
      await client.plots.setFlag(args.plotName, args.setFlag.flagId, args.setFlag.description);
    }
    if (args.removeFlag) {
      await client.plots.removeFlag(args.plotName, args.removeFlag);
    }
    if (args.branchTo) {
      await client.plots.branchTo(args.plotName, args.branchTo);
    }
    if (args.unbranch) {
      await client.plots.unbranch(args.plotName, args.unbranch);
    }

    return JSON.stringify({ updated: args.plotName });
  }, TOOL_NAMES.EDIT_PLOT),
});
