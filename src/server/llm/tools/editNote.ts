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
  noteName: z.string().describe("The name of the note (used as lookup key)."),
  action: z.enum(NOTE_ACTIONS).default("CREATE").describe("Action taken for the note."),
  // .nullable() is needed because LLMs often output null for omitted optional fields
  content: z
    .string()
    .nullable()
    .optional()
    .describe(
      `
Note text. CREATE: required. UPDATE: optional (set to overwrite). DELETE: omit.`.trim(),
    ),
  aboutEntities: z
    .array(z.string())
    .nullable()
    .optional()
    .describe(
      "Entity names to link this note to. Replaces existing links — pass [] to clear all.",
    ),
  aboutMessages: z
    .array(z.string())
    .nullable()
    .optional()
    .describe(
      "Message IDs to link this note to. Replaces existing links — pass [] to clear all. Link to messages to anchor notes to TimePoints via :Message AT_TIME → :TimePoint.",
    ),
});

export const editNote = tool({
  title: TOOL_NAMES.EDIT_NOTE,
  description: `
Your scratchpad — CREATE, UPDATE (partial overwrite), or DELETE a note. Notes can be
linked to entities (aboutEntities) and messages (aboutMessages) for cross-referencing to
the world and timeline.

Write a note when: tracking a suspicion or theory, an NPC made a promise/plan/threat,
a clue appeared but its meaning is unresolved, a player choice deserves future consequence.
A good note reads like a reminder to yourself: "Kael promised info about the glass cage.
Player paid 50 coins. Should reappear in 2-3 turns."

Search your notes via searchWorld at the start of every turn to recall what you were tracking.
`.trim(),
  inputSchema,
  execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
    const client = MemoryClient.getCachedInstance();

    if (args.action == "DELETE") {
      const deleted = await client.notes.deleteNote(args.noteName);
      return deleted
        ? `Note "${args.noteName}" is successfully deleted`
        : `ERROR: Note "${args.noteName}" is not found.`;
    }

    if (args.action == "CREATE") {
      if (!args.content) return `ERROR: Parameter "content" is required for CREATE.`;
      const note = await client.notes.createNote(args.noteName, args.content);
      if (args.aboutEntities) {
        for (const name of args.aboutEntities) await client.notes.linkToEntity(note.name, name);
      }
      if (args.aboutMessages) {
        for (const id of args.aboutMessages) await client.notes.linkToMessage(note.name, id);
      }
      return `Note "${note.name}" is successfully created (${note.content.length} chars, ${args.aboutEntities?.length ?? 0} entities linked, ${args.aboutMessages?.length ?? 0} messages linked).`;
    }

    const existing = await client.notes.getNote(args.noteName);
    if (!existing) return `ERROR: Note "${args.noteName}" not found.`;

    let flags = 0x0;
    // != null catches both null and undefined (LLM may output null for omitted fields).
    if (args.content != null) {
      flags |= 0x1;
      await client.notes.updateNote(args.noteName, { content: args.content });
    }
    if (args.aboutEntities != null) {
      flags |= 0x2;
      await client.notes.clearLinks(args.noteName, "ENTITY");
      for (const name of args.aboutEntities) await client.notes.linkToEntity(args.noteName, name);
    }
    if (args.aboutMessages != null) {
      flags |= 0x4;
      await client.notes.clearLinks(args.noteName, "MESSAGE");
      for (const id of args.aboutMessages) await client.notes.linkToMessage(args.noteName, id);
    }

    return `Note "${args.noteName}" is successfully updated (${[flags & 0x1 ? "content" : "", flags & 0x2 ? "all entities links" : "", flags & 0x4 ? "all messages links" : ""].join(", ")} is overwritten).`;
  }, TOOL_NAMES.EDIT_NOTE),
});
