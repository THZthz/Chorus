import { tool } from "ai";
import { updateWorldStateTool } from "./updateWorldState.ts";

export const createDraftUpdateWorldStateTool = (drafts: any) => tool({
  ...updateWorldStateTool,
  description: "Propose updates to world state entities. " + updateWorldStateTool.description,
  execute: async (args: any) => {
    if (args && args.updates) drafts.worldUpdates.push(...args.updates);
    return "Draft recorded.";
  }
});
