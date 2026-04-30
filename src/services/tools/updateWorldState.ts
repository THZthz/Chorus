import { tool } from "ai";
import { z } from "zod";
import { updateEntity } from "@/server/models/world";
import type { TurnEventEmitter } from "@/server/sseEvents";

const inputSchema = z.object({
  updates: z.array(z.object({
    id: z.string().describe("The unique ID of the entity to update (e.g., 'madam_vespera')."),
    longDescription: z.string().nullish().describe("New detailed observation."),
    shortDescription: z.string().nullish().describe("New concise label."),
    attributes: z.record(z.string(), z.string()).nullish().describe("Physical or mental traits."),
    opinions: z.record(z.string(), z.string()).nullish().describe("How they feel about the player or others."),
  })).describe("State changes to persist in the world memory."),
});

export const createUpdateWorldStateTool = (events: TurnEventEmitter) => tool({
  title: "Update World State",
  description: "Commit changes to world entities (characters, locations, objects). Updates descriptions, attributes, or opinions directly in the database.",
  inputSchema,
  execute: async (args: { updates: Array<{ id: string; longDescription?: string | null; shortDescription?: string | null; attributes?: Record<string, string> | null; opinions?: Record<string, string> | null }> }) => {
    for (const u of args.updates) {
      updateEntity(u);
      events.emitWorldUpdate(u.id, {
        ...(u.longDescription !== undefined ? { longDescription: u.longDescription } : {}),
        ...(u.shortDescription !== undefined ? { shortDescription: u.shortDescription } : {}),
        ...(u.attributes ? { attributes: u.attributes } : {}),
        ...(u.opinions ? { opinions: u.opinions } : {}),
      });
    }
    return `Updated ${args.updates.length} entities.`;
  },
});
