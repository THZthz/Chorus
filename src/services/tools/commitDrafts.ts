import { tool } from "ai";
import { z } from "zod";
import { updateEntity } from "@/server/models/world";
import { addPlot, updatePlotStatus } from "@/server/models/plot";
import { addMessage } from "@/server/models/history";

export const createCommitDraftsTool = (callbacks: {
  setFinished: (f: boolean) => void,
  setFinalResponse: (d: any) => void,
  drafts: any
}) => tool({
  title: "Commit Drafts",
  description: "Use this if the drafts are perfect and you want to commit them.",
  inputSchema: z.object({
    reasoning: z.string().describe("A brief explanation of why these drafts are ready to be committed.")
  }),
  execute: async ({ reasoning }: { reasoning: string }) => {
    try {
      console.log("Committing drafts because: ", reasoning);
      for (const u of callbacks.drafts.worldUpdates) updateEntity(u);
      for (const p of callbacks.drafts.newPlots) addPlot(p);
      for (const ps of callbacks.drafts.plotStatusUpdates) updatePlotStatus(ps.id, ps.status);

      // Attempt to save AI dialogue messages to DB directly here so we catch constraint violations
      if (callbacks.drafts.dialogue && callbacks.drafts.dialogue.messages) {
        for (let i = 0; i < callbacks.drafts.dialogue.messages.length; i++) {
          const msg = callbacks.drafts.dialogue.messages[i];
          addMessage({
            ...msg,
            id: msg.id || `ai-${Date.now()}-${i}-${Math.random().toString(36).substr(2, 9)}`
          });
        }
      }

      callbacks.setFinished(true);
      callbacks.setFinalResponse(callbacks.drafts.dialogue);
      return "Committed.";
    } catch (error: any) {
      console.error("Error committing drafts:", error);
      callbacks.setFinished(false);
      return `
Failed to commit drafts due to database or backend error: ${error.message}. You must review the drafts and tell the GM to fix the error. DO NOT try to commit again until the GM provides new drafts.
`.trim();
    }
  }
});
