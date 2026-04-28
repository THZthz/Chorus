import { tool } from "ai";
import { addPlotTool } from "./addPlot.ts";

export const createDraftAddPlotTool = (drafts: any) => tool({
  ...addPlotTool,
  description: "Propose a new concrete plot. " + addPlotTool.description,
  execute: async (args: any) => {
    drafts.newPlots.push({
      title: args.title,
      description: args.description,
      triggerCondition: args.triggerCondition
    });
    return "Draft recorded.";
  }
});
