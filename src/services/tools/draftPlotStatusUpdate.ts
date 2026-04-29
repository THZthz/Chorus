import { tool } from "ai";
import { z } from "zod";

const inputSchema = z.object({
  id: z.string().describe("The ID of the plot to update."),
  status: z.enum(['PENDING', 'IN_PROGRESS', 'RESOLVED']).describe("The new status of the plot.")
});

export const createDraftPlotStatusUpdateTool = (drafts: any) => tool({
  title: "Draft Plot Status Update",
  description: "Propose an advance to a plot status. Update the status of an existing plot (e.g. to IN_PROGRESS or RESOLVED)",
  inputSchema,
  execute: async (args: any) => {
    drafts.plotStatusUpdates.push({ id: args.id, status: args.status });
    return "Draft recorded.";
  }
});
