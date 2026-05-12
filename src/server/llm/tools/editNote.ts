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
import { MemoryClient } from "@/server/memory/client";
import { wrapSafe } from "@/server/llm/tools/shared";

export const editNote = tool({
  description:
    "Create, update, or delete a GM scratchpad note. Notes can be linked to entities and messages. Omit noteId to create. Set remove:true with noteId to delete.",
  inputSchema: z.object({
    noteId: z.string().optional().describe("Note ID to update/delete. Omit to create."),
    remove: z.boolean().default(false).describe("Set true to delete this note (requires noteId)."),
    content: z.string().optional().describe("Note text. Required for create, optional for update."),
    aboutEntities: z
      .array(z.string())
      .optional()
      .describe("Entity names to link this note to (replaces existing links)."),
    aboutMessages: z
      .array(z.string())
      .optional()
      .describe("Message IDs to link this note to (replaces existing links)."),
  }),
  execute: wrapSafe(async (args) => {
    const client = MemoryClient.getCachedInstance();

    if (args.noteId && args.remove) {
      const deleted = await client.notes.deleteNote(args.noteId);
      return JSON.stringify(deleted ? { removed: args.noteId } : { error: "Note not found" });
    }

    if (!args.noteId) {
      if (!args.content) return JSON.stringify({ error: "content required for create" });
      const note = await client.notes.createNote(args.content);
      if (args.aboutEntities)
        for (const name of args.aboutEntities) await client.notes.linkToEntity(note.id, name);
      if (args.aboutMessages)
        for (const id of args.aboutMessages) await client.notes.linkToMessage(note.id, id);
      return JSON.stringify({
        created: note.id,
        content: note.content,
        linkedEntities: args.aboutEntities?.length ?? 0,
        linkedMessages: args.aboutMessages?.length ?? 0,
      });
    }

    const existing = await client.notes.getNote(args.noteId);
    if (!existing) return JSON.stringify({ error: `Note "${args.noteId}" not found` });

    if (args.content) await client.notes.updateNote(args.noteId, { content: args.content });

    if (args.aboutEntities !== undefined || args.aboutMessages !== undefined) {
      await client.notes.clearLinks(args.noteId);
      if (args.aboutEntities)
        for (const name of args.aboutEntities) await client.notes.linkToEntity(args.noteId, name);
      if (args.aboutMessages)
        for (const id of args.aboutMessages) await client.notes.linkToMessage(args.noteId, id);
    }

    return JSON.stringify({ updated: args.noteId });
  }, "editNote"),
});
