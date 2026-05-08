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
import { removeFact } from "@/server/models/facts";
import type { TurnEventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  id: z.string().describe("ID of the fact to remove."),
});

export function createRemoveFactTool(events: TurnEventEmitter) {
  return tool({
    title: "Remove Fact",
    description:
      "Soft-delete a fact by ID. The fact is marked invalid but retained in the database. Reports an error if the fact ID does not exist.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const result = removeFact(args.id);
      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }
      events.emitFactRemove(args.id);
      return `Fact '${args.id}' removed.`;
    }, TOOL_NAMES.REMOVE_FACT),
  });
}
