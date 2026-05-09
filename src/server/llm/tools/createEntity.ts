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
import { upsertEntity } from "@/server/models/world";
import { nextId } from "@/server/models/ids";
import { getSceneState, setSceneState } from "@/server/models/scene";
import type { EventEmitter } from "@/server/llm/events";
import { ENTITY_TYPES, type WorldEntity } from "@/types/entities";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  type: z.enum(ENTITY_TYPES).describe("Type of entity to create."),
  displayName: z.string().describe("Display name for the entity."),
  shortDescription: z.string().describe("One-line summary."),
  longDescription: z.string().describe("Detailed narrative description."),
  attributes: z
    .record(z.string(), z.union([z.string(), z.number(), z.boolean()]))
    .optional()
    .describe("Physical or mental traits."),
  stats: z.record(z.string(), z.number()).optional().describe("Character stats (CHARACTER only)."),
  opinions: z
    .record(z.string(), z.string())
    .optional()
    .describe("Opinions about others as JSON keyed by characterId (CHARACTER only)."),
  conditions: z
    .record(z.string(), z.union([z.string(), z.number(), z.boolean()]))
    .optional()
    .describe("Status effects (CHARACTER only)."),
  initialLocationId: z
    .string()
    .optional()
    .describe("If set, place this entity at this location in the scene."),
});

export function createCreateEntityTool(events: EventEmitter) {
  return tool({
    title: "Create Entity",
    description:
      "Create a new world entity (character, location, or object). Generates an ID automatically. Optionally set an initial scene position via initialLocationId.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const entityId = `entity_${nextId()}`;

      const entity: any = {
        id: entityId,
        type: args.type,
        displayName: args.displayName,
        shortDescription: args.shortDescription,
        longDescription: args.longDescription,
        attributes: args.attributes || {},
      };

      if (args.type === "CHARACTER") {
        entity.stats = args.stats || {};
        entity.opinions = args.opinions || {};
        entity.conditions = args.conditions || {};
      }

      upsertEntity(entity as WorldEntity);

      if (args.initialLocationId) {
        const scene = getSceneState();
        if (args.type === "CHARACTER") {
          scene.characterLocations[entityId] = args.initialLocationId;
        } else if (args.type === "OBJECT") {
          scene.objectPositions[entityId] = {
            type: "location",
            locationId: args.initialLocationId,
          };
        }
        setSceneState(scene);
        events.emitSceneUpdate(scene);
      }

      events.emitEntityCreate(entityId, args.type, args.displayName);
      const locInfo = args.initialLocationId ? ` at location '${args.initialLocationId}'` : "";
      return `Entity created: "${args.displayName}" (${entityId}, ${args.type})${locInfo}.`;
    }, TOOL_NAMES.CREATE_ENTITY),
  });
}
