import { tool } from "ai";
import { z } from "zod";
import { getNoteById, getNotes, getNotesByIds } from "@/server/models/notes";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  id: z.string().optional().describe("Exact note ID to fetch."),
  ids: z.array(z.string()).optional().describe("Array of note IDs for bulk fetch."),
  relatedEntityId: z.string().optional().describe("Filter notes linked to this entity ID."),
  relatedPlotId: z.string().optional().describe("Filter notes linked to this plot ID."),
  relatedScene: z.boolean().optional().describe("Filter notes linked (or not) to scene state."),
  relatedTime: z.boolean().optional().describe("Filter notes linked (or not) to game time."),
});

export function createGetNoteTool() {
  return tool({
    title: "Get Note",
    description:
      "Retrieve notes: by single ID, multiple IDs (bulk), or filter by related entity, plot, scene, or time. Only returns valid (non-removed) notes.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      if (args.id) {
        const note = getNoteById(args.id);
        if (!note || !note.isValid) {
          return `ERROR: Note '${args.id}' not found.`;
        }
        return JSON.stringify(note, null, 2);
      }

      if (args.ids && args.ids.length > 0) {
        const notes = getNotesByIds(args.ids);
        if (notes.length === 0) {
          return `No valid notes found for the provided IDs: [${args.ids.join(", ")}].`;
        }
        const found = new Set(notes.map((n) => n.id));
        const missing = args.ids.filter((id) => !found.has(id));
        const result: Record<string, unknown> = { notes };
        if (missing.length > 0) result.missingIds = missing;
        return JSON.stringify(result, null, 2);
      }

      const notes = getNotes({
        relatedEntityId: args.relatedEntityId,
        relatedPlotId: args.relatedPlotId,
        relatedScene: args.relatedScene,
        relatedTime: args.relatedTime,
      });
      if (notes.length === 0) return "No notes found matching the filter.";
      return JSON.stringify(notes, null, 2);
    }, TOOL_NAMES.GET_NOTE),
  });
}
