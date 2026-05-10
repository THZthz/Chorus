/**
 * Elysian Dialogue — cinematic RPG-style dialogue engine
 * Copyright (C) 2026  Amias
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

import { tool } from "ai";
import { z } from "zod";
import { getNoteById, getNotes, getNotesByIds } from "@/server/models/notes";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe, formatNoteMarkdown } from "@/server/llm/tools/shared";

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
        return formatNoteMarkdown(note);
      }

      if (args.ids && args.ids.length > 0) {
        const notes = getNotesByIds(args.ids);
        if (notes.length === 0) {
          return `No valid notes found for the provided IDs: [${args.ids.join(", ")}].`;
        }
        const found = new Set(notes.map((n) => n.id));
        const missing = args.ids.filter((id) => !found.has(id));
        const parts: string[] = [];
        if (missing.length > 0) {
          parts.push(`> Note: The following IDs were not found: [${missing.join(", ")}]`);
          parts.push("");
        }
        parts.push(`## Notes (${notes.length} results)`);
        parts.push("");
        for (const note of notes) {
          parts.push(formatNoteMarkdown(note));
          parts.push("");
          parts.push("---");
          parts.push("");
        }
        return parts.join("\n").trim();
      }

      const notes = getNotes({
        relatedEntityId: args.relatedEntityId,
        relatedPlotId: args.relatedPlotId,
        relatedScene: args.relatedScene,
        relatedTime: args.relatedTime,
      });
      if (notes.length === 0) return "No notes found matching the filter.";
      const parts: string[] = [`## Notes (${notes.length} results)`, ""];
      for (const note of notes) {
        parts.push(formatNoteMarkdown(note));
        parts.push("");
        parts.push("---");
        parts.push("");
      }
      return parts.join("\n").trim();
    }, TOOL_NAMES.GET_NOTE),
  });
}
