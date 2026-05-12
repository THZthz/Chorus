import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { wrapSafe } from "@/server/llm/tools/shared";

export const editPlot = tool({
  description: "Create, update, or delete a plot. Plots track story arcs. Use flags for player knowledge. Use branchTo/unbranch to connect child plots. Plots are separate from world entities — use searchPlots to find them.",
  inputSchema: z.object({
    plotName: z.string().optional().describe("Plot name. Omit to create, include to update/delete."),
    remove: z.boolean().default(false).describe("Set true to delete (requires plotName)."),
    description: z.string().optional().describe("Plot description."),
    status: z.enum(["PENDING", "ACTIVE", "IN_PROGRESS", "COMPLETED", "ABANDONED"]).optional().describe("Plot status."),
    triggerCondition: z.string().optional().describe("Condition that activates this plot."),
    setFlag: z.object({ flagId: z.string(), description: z.string() }).optional().describe("Add or update a player flag on this plot."),
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

    const updates: Record<string, unknown> = {};
    if (args.description !== undefined) updates.description = args.description;
    if (args.status !== undefined) updates.status = args.status;
    if (args.triggerCondition !== undefined) updates.triggerCondition = args.triggerCondition;

    if (Object.keys(updates).length > 0) {
      await client.plots.updatePlot(args.plotName, updates as any);
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
  }, "editPlot"),
});
