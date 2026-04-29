import { tool } from "ai";
import { z } from "zod";

const inputSchema = z.object({
  title: z.string().describe("Concise title of the plot/quest."),
  description: z.string().describe("Detailed description of what the plot is about."),
  triggerCondition: z.string().describe("The specific condition or scene that triggers this plot (e.g. 'Enter the tavern').")
});

export const createDraftPlotTool = (drafts: any) => tool({
  title: "Draft Plot",
  description: "Propose a new concrete plot. Introduce a new concrete plot in a new location, specifying a clear trigger condition.",
  inputSchema,
  execute: async (args: any) => {
    drafts.newPlots.push({
      title: args.title,
      description: args.description,
      triggerCondition: args.triggerCondition
    });
    return "Draft recorded.";
  }
});
