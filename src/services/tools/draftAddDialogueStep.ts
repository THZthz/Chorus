import { tool } from "ai";
import { addDialogueStepTool } from "./addDialogueStep.ts";

export const createDraftAddDialogueStepTool = (drafts: any) => tool({
  ...addDialogueStepTool,
  description: "Propose a narrative dialogue step. " + addDialogueStepTool.description,
  execute: async (args: any) => {
    drafts.dialogue = args;
    return "Draft recorded.";
  }
});
