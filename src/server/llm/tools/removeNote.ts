import { tool } from "ai";
import { z } from "zod";
import { removeNote } from "@/server/models/notes";
import type { EventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  id: z.string().describe("ID of the note to remove."),
});

export function createRemoveNoteTool(events: EventEmitter) {
  return tool({
    title: "Remove Note",
    description:
      "Soft-delete a note by ID. The note is marked invalid but retained in the database. Reports an error if the note ID does not exist.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const result = removeNote(args.id);
      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }
      events.emitNoteRemove(args.id);
      return `Note '${args.id}' removed.`;
    }, TOOL_NAMES.REMOVE_NOTE),
  });
}
