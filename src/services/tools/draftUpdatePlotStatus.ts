import { tool } from "ai";
import { updatePlotStatusTool } from "./updatePlotStatus.ts";

export const createDraftUpdatePlotStatusTool = (drafts: any) => tool({
  ...updatePlotStatusTool,
  description: "Propose an advance to a plot status. " + updatePlotStatusTool.description,
  execute: async (args: any) => {
    drafts.plotStatusUpdates.push({ id: args.id, status: args.status });
    return "Draft recorded.";
  }
});
