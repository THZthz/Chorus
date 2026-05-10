import { tool } from "ai";
import { z } from "zod";
import { updateNote } from "@/server/models/notes";
import type { EventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  id: z.string().describe("ID of the note to update."),
  key: z.string().optional().describe("New key label."),
  value: z.string().optional().describe("New value."),
  relatedEntityIds: z
    .array(z.string())
    .optional()
    .describe("Replacement list of related entity IDs."),
  relatedPlotIds: z.array(z.string()).optional().describe("Replacement list of related plot IDs."),
  relatedScene: z.boolean().optional().describe("Whether this relates to scene state."),
  relatedTime: z.boolean().optional().describe("Whether this relates to game time."),
});

export function createUpdateNoteTool(events: EventEmitter) {
  return tool({
    title: "Update Note",
    description:
      "Update an existing note's key, value, or related links. Only valid notes can be updated. Reports an error if the note ID does not exist.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const result = updateNote(args.id, {
        key: args.key,
        value: args.value,
        relatedEntityIds: args.relatedEntityIds,
        relatedPlotIds: args.relatedPlotIds,
        relatedScene: args.relatedScene,
        relatedTime: args.relatedTime,
      });

      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }

      const changes: Record<string, unknown> = {};
      if (args.key !== undefined) changes.key = args.key;
      if (args.value !== undefined) changes.value = args.value;
      if (args.relatedEntityIds !== undefined) changes.relatedEntityIds = args.relatedEntityIds;
      if (args.relatedPlotIds !== undefined) changes.relatedPlotIds = args.relatedPlotIds;
      if (args.relatedScene !== undefined) changes.relatedScene = args.relatedScene;
      if (args.relatedTime !== undefined) changes.relatedTime = args.relatedTime;
      events.emitNoteUpdate(args.id, changes);

      return `Note "${result.note.key}" (${args.id}) updated.`;
    }, TOOL_NAMES.UPDATE_NOTE),
  });
}
