import { tool } from "ai";
import { z } from "zod";

// WARNING: Do not remove this comment! Keep schema in sync with "/types/dialogue.ts"!
const inputSchema = z.object({
  id: z.string().describe("A unique ID for this generated dialogue step (e.g., 'scene_aftermath_1')."),
  messages: z.array(z.object({
    id: z.string().describe("A REQUIRED, globally unique ID for this specific message (e.g., 'msg_aftermath_1'). NEVER duplicate this across messages or turns."),
    speaker: z.string().describe("Character name, or one of the internal voices: 'LOGIC', 'RHETORIC', 'VOLITION', 'INLAND EMPIRE', 'HALF LIGHT', 'ELECTROCHEMISTRY', etc. Use 'INNER_VOICE' for generic psyce commentary."),
    type: z.enum(["YOU", "INNER_VOICE", "CHARACTER", "SYSTEM", "ROLL"]).describe("The category of the message. Use 'INNER_VOICE' for all internal voices/thoughts."),
    text: z.string().describe("Markdown supported. Use [Object Name](#object_id) for reactive world entities. Emphasize physical sensations and philosophical dread."),
    skillCheck: z.object({
      skill: z.string().describe("The name of the internal voice or skill that was just passively checked."),
      difficulty: z.string().describe("Difficulty label (e.g., 'Challenging', 'Easy')."),
      success: z.boolean().describe("Result of the passive check.")
    }).nullish().describe("Only include if the AI decides a passive skill check just happened.")
  })).describe("The narrative flow of the response. Use multiple messages for pacing, alternating between external dialogue and inner monologue."),
  options: z.array(z.object({
    id: z.string().describe("Unique option ID (e.g., 'opt_demand_truth')."),
    text: z.string().describe("User-facing label. Pure narrative text only."),
    hintBefore: z.string().nullish().describe("Optional bracketed hint before text, e.g. '[Consult the Void]'."),
    hintAfter: z.string().nullish().describe("Optional bracketed hint after text, e.g. '[Charm her.]'."),
    isAiTrigger: z.boolean().describe("Set to TRUE to continue the dynamic AI conversation loop. Set to FALSE to jump to static content."),
    nextStepId: z.string().nullish().describe("Required if isAiTrigger is FALSE. Must match a static scene ID like 'start' or 'leave'."),
    check: z.object({
      skill: z.string().describe("The internal name of the skill used for the active check."),
      difficulty: z.number().describe("The numeric target to roll (e.g., 10, 15)."),
      difficultyText: z.string().describe("The flavor name for the difficulty (e.g., 'Hard')."),
      diceCount: z.number().describe("Usually 2 (rolling 2d6)."),
      conditions: z.array(z.object({
        expression: z.string().describe("JS condition: 'total' (dice sum), 'success' (total >= difficulty), and 'dice' (array). Example: 'success', 'dice[0] === 1 && dice[1] === 1'."),
        stepId: z.string().describe("The ID of the generated step to move to if met."),
        label: z.string().nullish().describe("Label for the roll outcome."),
        color: z.string().nullish().describe("Tailwind color class (e.g., 'text-red-500').")
      }))
    }).nullish().describe("Active skill checks the player must roll for.")
  })).describe("Suggestions for the player's next move.")
});

export const createDraftDialogueStepTool = (drafts: any) => tool({
  title: "Draft Dialogue Step",
  description: "Propose a narrative dialogue step. Adds a new dialogue step with messages and options for the player. This tool call represents the end of your response.",
  inputSchema: inputSchema,
  execute: async (args: any) => {
    drafts.dialogue = args;
    return "Draft recorded.";
  }
});
