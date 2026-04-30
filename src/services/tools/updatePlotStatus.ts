import { tool } from "ai";
import { z } from "zod";
import { updatePlotStatus as updatePlotStatusDb } from "@/server/models/plot";
import type { TurnEventEmitter } from "@/server/sseEvents";

const inputSchema = z.object({
  id: z.string().describe("The ID of the plot to update."),
  status: z.enum(["PENDING", "IN_PROGRESS", "RESOLVED"]).describe("The new status of the plot."),
});

export const createUpdatePlotStatusTool = (events: TurnEventEmitter) => tool({
  title: "Update Plot Status",
  description: "Update the status of an existing plot (e.g., to IN_PROGRESS or RESOLVED). Commits directly to the database.",
  inputSchema,
  execute: async (args: { id: string; status: string }) => {
    updatePlotStatusDb(args.id, args.status);
    events.emitPlotUpdate(args.id, args.status);
    return `Plot ${args.id} status updated to ${args.status}.`;
  },
});
