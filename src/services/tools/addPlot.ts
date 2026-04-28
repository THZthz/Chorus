import { tool } from "ai";
import { z } from "zod";

export const addPlotTool = tool({
  description: "Introduce a new concrete plot in a new location, specifying a clear trigger condition.",
  inputSchema: z.object({
    title: z.string().describe("Concise title of the plot/quest."),
    description: z.string().describe("Detailed description of what the plot is about."),
    triggerCondition: z.string().describe("The specific condition or scene that triggers this plot (e.g. 'Enter the tavern').")
  })
});
