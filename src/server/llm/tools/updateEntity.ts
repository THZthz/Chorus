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
import { updateEntity, getEntityById } from "@/server/models/world";
import type { EventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  id: z.string().describe("The unique ID of the entity to update (e.g. 'madam_vespera')."),
  shortDescription: z.string().optional().describe("New concise label."),
  longDescription: z.string().optional().describe("New detailed observation."),
  attributes: z
    .record(z.string(), z.string())
    .optional()
    .describe("Physical or mental traits (merged)."),
  opinions: z
    .record(z.string(), z.string())
    .optional()
    .describe("How this character feels about others (merged). Only valid for CHARACTER entities."),
});

export function createUpdateEntityTool(events: EventEmitter) {
  return tool({
    title: "Update Entity",
    description:
      "Mutate a single world entity's description, attributes, or opinions. One entity per call. Reports an error if the entity ID does not exist.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const existing = getEntityById(args.id);
      if (!existing) {
        return `ERROR: Entity '${args.id}' not found. You may use ${TOOL_NAMES.LIST_ENTITIES}() to discover valid IDs.`;
      }
      updateEntity(args);
      const changes: Record<string, unknown> = {};
      if (args.longDescription != null) changes.longDescription = args.longDescription;
      if (args.shortDescription != null) changes.shortDescription = args.shortDescription;
      if (args.attributes) changes.attributes = args.attributes;
      if (args.opinions) changes.opinions = args.opinions;
      events.emitWorldUpdate(args.id, changes);
      return `Entity with name '${existing.displayName}' (id: ${args.id}) updated.`;
    }, TOOL_NAMES.UPDATE_ENTITY),
  });
}
