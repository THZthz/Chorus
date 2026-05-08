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
import { getSceneState, setSceneState } from "@/server/models/scene";
import type { TurnEventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  currentLocationId: z
    .string()
    .optional()
    .describe("Change the current scene's location to this entity ID."),
  moveCharacters: z
    .array(
      z.object({
        characterId: z.string().describe("Character entity ID to move."),
        locationId: z.string().describe("Destination location entity ID."),
      }),
    )
    .optional()
    .describe("Characters to relocate to a different location."),
  moveObjects: z
    .array(
      z.object({
        objectId: z.string().describe("Object entity ID to move."),
        toLocationId: z.string().optional().describe("Location entity ID to place the object at."),
        toCharacterId: z
          .string()
          .optional()
          .describe("Character entity ID to give the object to (carried)."),
      }),
    )
    .optional()
    .describe(
      "Objects to move. Provide toLocationId to place at a location, or toCharacterId to give to a character (not both).",
    ),
});

export function createUpdateSceneTool(events: TurnEventEmitter) {
  return tool({
    title: "Update Scene",
    description:
      "Update the current scene: change the active location, move characters between locations, or move objects (to a location or into a character's possession). All fields are optional — only specified changes are applied. The scene tracks who is where and who is carrying what.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const scene = getSceneState();
      const changes: string[] = [];

      if (args.currentLocationId) {
        scene.currentLocationId = args.currentLocationId;
        changes.push(`Current location set to '${args.currentLocationId}'.`);
      }

      if (args.moveCharacters) {
        for (const { characterId, locationId } of args.moveCharacters) {
          scene.characterLocations[characterId] = locationId;
          changes.push(`Moved character '${characterId}' to location '${locationId}'.`);
        }
      }

      if (args.moveObjects) {
        for (const { objectId, toLocationId, toCharacterId } of args.moveObjects) {
          if (toLocationId && toCharacterId) {
            return `ERROR: Object '${objectId}' has both toLocationId and toCharacterId — choose one.`;
          }
          if (!toLocationId && !toCharacterId) {
            return `ERROR: Object '${objectId}' needs either toLocationId or toCharacterId.`;
          }
          if (toLocationId) {
            scene.objectPositions[objectId] = {
              type: "location",
              locationId: toLocationId,
            };
            changes.push(`Placed object '${objectId}' at location '${toLocationId}'.`);
          } else {
            scene.objectPositions[objectId] = {
              type: "character",
              characterId: toCharacterId!,
            };
            changes.push(`Gave object '${objectId}' to character '${toCharacterId}'.`);
          }
        }
      }

      setSceneState(scene);
      events.emitSceneUpdate(scene);

      if (changes.length === 0) {
        return "Scene unchanged. No fields were specified.";
      }
      return `Scene updated:\n${changes.map((c) => `- ${c}`).join("\n")}`;
    }, TOOL_NAMES.UPDATE_SCENE),
  });
}
