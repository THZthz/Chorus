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
import { addFact } from "@/server/models/facts";
import type { EventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  key: z.string().describe("Short label for the fact (e.g. 'player_suspects_cressida')."),
  value: z.string().describe("The fact value — what the GM needs to remember."),
  relatedEntityIds: z.array(z.string()).optional().describe("Entity IDs this fact relates to."),
  relatedPlotIds: z.array(z.string()).optional().describe("Plot IDs this fact relates to."),
  relatedScene: z
    .boolean()
    .optional()
    .describe("Set true if this fact relates to the current scene state."),
  relatedTime: z
    .boolean()
    .optional()
    .describe("Set true if this fact relates to the current game time."),
});

export function createAddFactTool(events: EventEmitter) {
  return tool({
    title: "Add Fact",
    description:
      "Record a GM fact — private working memory that persists between turns. Use this to remember narrative state that isn't a plot: suspicions, countdowns, character relationship changes, environmental details, etc. Facts link to related entities, plots, scene, or time for filtering.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const fact = addFact({
        key: args.key,
        value: args.value,
        relatedEntityIds: args.relatedEntityIds,
        relatedPlotIds: args.relatedPlotIds,
        relatedScene: args.relatedScene,
        relatedTime: args.relatedTime,
      });
      events.emitFactAdd(fact);
      return `Fact recorded: "${args.key}" (${fact.id}).`;
    }, TOOL_NAMES.ADD_FACT),
  });
}
