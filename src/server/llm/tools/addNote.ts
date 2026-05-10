import { tool } from "ai";
import { z } from "zod";
import { addNote } from "@/server/models/notes";
import type { EventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  key: z.string().describe("Short label for the note (e.g. 'player_suspects_cressida')."),
  value: z.string().describe("The note value — what the GM needs to remember."),
  relatedEntityIds: z.array(z.string()).optional().describe("Entity IDs this note relates to."),
  relatedPlotIds: z.array(z.string()).optional().describe("Plot IDs this note relates to."),
  relatedScene: z
    .boolean()
    .optional()
    .describe("Set true if this note relates to the current scene state."),
  relatedTime: z
    .boolean()
    .optional()
    .describe("Set true if this note relates to the current game time."),
});

export function createAddNoteTool(events: EventEmitter) {
  return tool({
    title: "Add Note",
    description:
      "Record a GM note — private scratchpad memory that persists between turns. Use this to remember narrative state that isn't a plot: suspicions, countdowns, character relationship changes, environmental details, etc. Notes link to related entities, plots, scene, or time for filtering.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const note = addNote({
        key: args.key,
        value: args.value,
        relatedEntityIds: args.relatedEntityIds,
        relatedPlotIds: args.relatedPlotIds,
        relatedScene: args.relatedScene,
        relatedTime: args.relatedTime,
      });
      events.emitNoteAdd(note);
      return `Note recorded: "${args.key}" (${note.id}).`;
    }, TOOL_NAMES.ADD_NOTE),
  });
}
