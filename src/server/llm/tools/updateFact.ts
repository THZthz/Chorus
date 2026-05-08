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
import { updateFact } from "@/server/models/facts";
import type { TurnEventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  id: z.string().describe("ID of the fact to update."),
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

export function createUpdateFactTool(events: TurnEventEmitter) {
  return tool({
    title: "Update Fact",
    description:
      "Update an existing fact's key, value, or related links. Only valid facts can be updated. Reports an error if the fact ID does not exist.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const result = updateFact(args.id, {
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
      events.emitFactUpdate(args.id, changes);

      return `Fact "${result.fact.key}" (${args.id}) updated.`;
    }, TOOL_NAMES.UPDATE_FACT),
  });
}
