import { tool } from "ai";
import { z } from "zod";

export const createReplyToGMTool = (callbacks: { setFeedback: (f: string) => void }) => tool({
  description: "Use this to reject the drafts and give feedback to the GM.",
  parameters: z.object({ feedback: z.string() }),
  execute: async ({ feedback }: { feedback: string }) => {
    callbacks.setFeedback(feedback);
    return "Feedback sent to GM.";
  }
});
