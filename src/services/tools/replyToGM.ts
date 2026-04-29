import { tool } from "ai";
import { z } from "zod";

export const createReplyToGMTool = (callbacks: { setFeedback: (f: string) => void }) => tool({
  title: "Reply to GM",
  description: "Use this to reject the drafts and give feedback to the GM.",
  inputSchema: z.object({ 
    feedback: z.string().describe("The specific reasons for rejection or requested changes for the GM.")
  }),
  execute: async ({ feedback }: { feedback: string }) => {
    callbacks.setFeedback(feedback);
    return "Feedback sent to GM.";
  }
});
