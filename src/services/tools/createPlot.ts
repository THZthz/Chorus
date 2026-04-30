import { tool } from "ai";
import { z } from "zod";
import { addPlot } from "@/server/models/plot";
import type { TurnEventEmitter } from "@/server/sseEvents";

const inputSchema = z.object({
  title: z.string().describe("Concise title of the plot/quest."),
  description: z.string().describe("Detailed description of what the plot is about."),
  triggerCondition: z.string().describe("The specific condition or scene that triggers this plot."),
});

export const createCreatePlotTool = (events: TurnEventEmitter) => tool({
  title: "Create Plot",
  description: "Create a new plot/quest. Commits directly to the database.",
  inputSchema,
  execute: async (args: { title: string; description: string; triggerCondition: string }) => {
    const plotId = `plot_${Date.now()}`;
    addPlot({ id: plotId, title: args.title, description: args.description, triggerCondition: args.triggerCondition });
    events.emitPlotCreate(plotId, args.title);
    return `Plot created: ${args.title} (${plotId}).`;
  },
});
