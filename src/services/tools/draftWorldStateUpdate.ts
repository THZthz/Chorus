import { tool } from "ai";
import { z } from "zod";

const inputSchema = z.object({
  updates: z.array(z.object({
    id: z.string().describe("The unique ID of the entity to update (e.g., 'madam_vespera')."),
    longDescription: z.string().nullish().describe("New detailed observation."),
    shortDescription: z.string().nullish().describe("New concise label."),
    attributes: z.record(z.string(), z.string()).nullish().describe("Physical or mental traits."),
    opinions: z.record(z.string(), z.string()).nullish().describe("How they feel about YOU (the player) or others.")
  })).describe("State changes to persist in the world memory."),
});

export const createDraftWorldStateUpdateTool = (drafts: any) => tool({
  title: "Draft World State Update",
  description: "Propose updates to world state entities. Updates the world state with new observations or character state changes. Call this to update opinions, descriptions, or attributes.",
  inputSchema,
  execute: async (args: any) => {
    if (args && args.updates) drafts.worldUpdates.push(...args.updates);
    return "Draft recorded.";
  }
});
