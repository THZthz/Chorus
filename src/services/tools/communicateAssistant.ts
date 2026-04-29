import { tool, generateText, stepCountIs, type LanguageModel } from "ai";
import { z } from "zod";
import { createReplyToGMTool } from "@/services/tools/replyToGM";
import { createCommitDraftsTool } from "@/services/tools/commitDrafts";

export const createCommunicateAssistantTool = (params: {
  model: LanguageModel;
  worldState: any;
  activePlots: any;
  drafts: any;
  onCommit: (dialogue: any) => void;
}) => tool({
  title: "Communicate Assistant",
  description: "Submit all drafted changes to the Assistant for review. You must provide a message. The Assistant will approve or request changes.",
  inputSchema: z.object({
    message: z.string().describe("The message to send to the assistant, summarizing your drafts and intent.")
  }),
  execute: async ({ message }: { message: string }) => {
    const assistantSystemInstruction = `
You are the vigilant Assistant to the Game Master.
Your job is to thoroughly review the drafts proposed by the Game Master.

### Structure of DialogueStep
The GM must propose a 'dialogue' draft that follows this structure:
\`\`\`typescript
interface Message {
  speaker: string;
  type: 'YOU' | 'INNER_VOICE' | 'CHARACTER' | 'SYSTEM' | 'ROLL' | 'NOTIFICATION';
  text: string;
  metadata?: { notificationType?: 'XP' | 'TASK' | 'ITEM' };
}

interface DialogueOption {
  text: string;
  nextStepId?: string; // Standard transition id
  isAiTrigger?: boolean; // Set to true if the option should trigger another GM response when clicked
  check?: {
    skill: string;
    difficulty: number;
    conditions: { expression: string; stepId: string; label?: string }[];
  };
}

interface DialogueStep {
  messages: Message[];
  options: DialogueOption[];
}
\`\`\`

Verification Checklist for GM Drafts:
1. **Dialogue Flow**: Does the 'dialogue' draft make sense in context?
2. **AI Triggers**: Options leading to generic "Let's talk more" moments should likely have 'isAiTrigger: true'.
3. **World State**: Did the GM mutate actors or items correctly?
4. **Consistency**: Ensure 'speaker' names match the World State.

Good tool usage:
- If everything is perfect, call 'commitDrafts'.
- If the GM made a mistake (like forgetting 'isAiTrigger: true', missing a plot trigger, mutating a non-existent item), use 'replyToGM' to tell them to fix it.

Bad tool usage:
- Responding with text only instead of calling tools.
`.trim();

    const assistantMessages: any[] = [
      {
        role: 'user',
        content: `
GM's Message: ${message}\n\nCurrent World State:\n${JSON.stringify(params.worldState, null, 2)}\n\nActive Plots:\n${JSON.stringify(params.activePlots, null, 2)}\n\nProposed Drafts:\n${JSON.stringify(params.drafts, null, 2)}\n\nEvaluate the drafts.
`.trim()
      }
    ];

    let assistantFeedback = "";
    let turnFinished = false;

    await generateText({
      model: params.model,
      system: assistantSystemInstruction,
      messages: assistantMessages,
      stopWhen: stepCountIs(3),
      tools: {
        replyToGM: createReplyToGMTool({
          setFeedback: (f) => { assistantFeedback = f; }
        }),
        commitDrafts: createCommitDraftsTool({
          setFinished: (f) => { turnFinished = f; },
          setFinalResponse: (d) => { params.onCommit(d); },
          drafts: params.drafts
        })
      }
    });

    if (turnFinished) {
      return "SUCCESS: Assistant committed the drafts. Turn is over.";
    } else {
      // Clear drafts as per original logic
      params.drafts.worldUpdates = [];
      params.drafts.newPlots = [];
      params.drafts.plotStatusUpdates = [];
      params.drafts.dialogue = null;

      return `
ASSISTANT FEEDBACK: ${assistantFeedback || "No feedback provided by assistant."}. Your drafts have been CLEARED. Please propose new ones and call communicateAssistant again.
`.trim();
    }
  }
});
