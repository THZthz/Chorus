import { tool } from "ai";
import { z } from "zod";
import { updateEntity } from "@/server/models/world";
import { addPlot, updatePlotStatus } from "@/server/models/plot";

export const createCommitDraftsTool = (callbacks: { 
  setFinished: (f: boolean) => void, 
  setFinalResponse: (d: any) => void,
  drafts: any 
}) => tool({
  description: "Use this if the drafts are perfect and you want to commit them.",
  inputSchema: z.object({
    reasoning: z.string().describe("A brief explanation of why these drafts are ready to be committed.")
  }),
  execute: async ({ reasoning }: { reasoning: string }) => {
    console.log("Committing drafts because: ", reasoning);
    callbacks.setFinished(true);
    for (const u of callbacks.drafts.worldUpdates) updateEntity(u);
    for (const p of callbacks.drafts.newPlots) addPlot(p);
    for (const ps of callbacks.drafts.plotStatusUpdates) updatePlotStatus(ps.id, ps.status);
    callbacks.setFinalResponse(callbacks.drafts.dialogue);
    return "Committed.";
  }
});
