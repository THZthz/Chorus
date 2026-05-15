/**
 * Chorus — cinematic RPG-style dialogue engine
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
import { TOOL_NAMES } from "@/shared/constants";

const NOTE_ACTIONS = ["CREATE", "UPDATE", "DELETE"] as const;

const inputSchema = z.object({
  noteName: z.string().describe("The name of target note."),
  action: z.enum(NOTE_ACTIONS).default("CREATE").describe("Action taken for the note."),
  content: z
    .string()
    .optional()
    .describe(
      `
Note text. CREATE: required; UPDATE: optional, set to overwrite old content; DELETE: omit.`.trim(),
    ),
  aboutEntities: z
    .array(z.string())
    .optional()
    .describe("Entity names to link this note to (replaces existing links)."),
  aboutMessages: z
    .array(z.string())
    .optional()
    .describe("Message IDs to link this note to (replaces existing links)."),
});

export const editNote = tool({
  title: TOOL_NAMES.EDIT_NOTE,
  description: `
Create, update, or delete a scratchpad note.
Notes can be linked to entities and messages.
`.trim(),
  inputSchema,
  execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
    const client = MemoryClient.getCachedInstance();

    if (args.action == "DELETE") {
      const deleted = await client.notes.deleteNote(args.noteName);
      return JSON.stringify(
        deleted ? { removed: args.noteName } : { error: `Note "${args.noteName}" not found.` },
      );
    }

    if (args.action == "CREATE") {
      if (!args.content) return JSON.stringify({ error: "content required for CREATE." });
      const note = await client.notes.createNote(args.noteName, args.content);
      if (args.aboutEntities) {
        for (const name of args.aboutEntities) await client.notes.linkToEntity(note.id, name);
      }
      if (args.aboutMessages) {
        for (const id of args.aboutMessages) await client.notes.linkToMessage(note.id, id);
      }
      return JSON.stringify({
        created: note.id,
        content: note.content,
        linkedEntities: args.aboutEntities?.length ?? 0,
        linkedMessages: args.aboutMessages?.length ?? 0,
      });
    }

    const existing = await client.notes.getNote(args.noteName);
    if (!existing) return JSON.stringify({ error: `Note "${args.noteName}" not found.` });

    if (args.content !== undefined) {
      await client.notes.updateNote(args.noteName, { content: args.content });
    }
    if (args.aboutEntities !== undefined && args.aboutEntities) {
      await client.notes.clearLinks(args.noteName, "ENTITY");
      for (const name of args.aboutEntities) await client.notes.linkToEntity(args.noteName, name);
    }
    if (args.aboutMessages !== undefined && args.aboutMessages) {
      await client.notes.clearLinks(args.noteName, "MESSAGE");
      for (const id of args.aboutMessages) await client.notes.linkToMessage(args.noteName, id);
    }

    return JSON.stringify({ updated: args.noteName });
  }, TOOL_NAMES.EDIT_NOTE),
});
