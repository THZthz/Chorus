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
import { getEntityById } from "@/server/models/world";
import { getSceneState } from "@/server/models/scene";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  characterId: z.string().describe("The character entity ID to query."),
});

export function createGetCharacterStateTool() {
  return tool({
    title: "Get Character State",
    description:
      "Get a character's full state: entity details, stats, opinions, conditions, and what objects they carry per the current scene.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const entity = getEntityById(args.characterId);
      if (!entity) {
        return `ERROR: Entity '${args.characterId}' not found.`;
      }
      if (entity.type !== "CHARACTER") {
        return `ERROR: Entity '${args.characterId}' is a ${entity.type}, not a CHARACTER.`;
      }
      const scene = getSceneState();
      const carriedObjects: string[] = [];
      for (const [objId, pos] of Object.entries(scene.objectPositions)) {
        if (pos.type === "character" && pos.characterId === args.characterId) {
          carriedObjects.push(objId);
        }
      }
      return JSON.stringify(
        {
          character: entity,
          carriedObjects,
          sceneLocation: scene.characterLocations[args.characterId] ?? null,
        },
        null,
        2,
      );
    }, TOOL_NAMES.GET_CHARACTER_STATE),
  });
}
