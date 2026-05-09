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
import { getSceneState, setSceneState } from "@/server/models/scene";
import type { EventEmitter } from "@/server/llm/events";
import type { Character } from "@/types/entities";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  characterId: z.string().describe("The character entity ID to update."),
  stats: z.record(z.string(), z.number()).optional().describe("Stat changes to merge."),
  conditions: z
    .record(z.string(), z.union([z.string(), z.number(), z.boolean(), z.null()]))
    .optional()
    .describe("Conditions to merge. Set a value to null to remove that condition."),
  carriedObjects: z
    .object({
      add: z.array(z.string()).optional().describe("Object IDs to give to this character."),
      remove: z.array(z.string()).optional().describe("Object IDs to remove from this character."),
    })
    .optional()
    .describe("Inventory changes (updates scene objectPositions)."),
});

export function createUpdateCharacterStateTool(events: EventEmitter) {
  return tool({
    title: "Update Character State",
    description:
      "Update a character's stats, conditions, or carried inventory. Stats are merged. Conditions are merged — set a key to null to remove it. Carried objects update scene objectPositions.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const existing = getEntityById(args.characterId);
      if (!existing) {
        return `ERROR: Entity '${args.characterId}' not found.`;
      }
      if (existing.type !== "CHARACTER") {
        return `ERROR: Entity '${args.characterId}' is a ${existing.type}, not a CHARACTER.`;
      }

      const changes: Record<string, unknown> = {};
      const resultParts: string[] = [];

      if (args.stats && Object.keys(args.stats).length > 0) {
        updateEntity({ id: args.characterId, stats: args.stats } as any);
        changes.stats = args.stats;
        resultParts.push(`stats updated: ${JSON.stringify(args.stats)}`);
      }

      if (args.conditions) {
        const char = existing as Character;
        const merged = { ...char.conditions };
        for (const [key, value] of Object.entries(args.conditions)) {
          if (value === null) {
            delete merged[key];
          } else {
            merged[key] = value;
          }
        }
        updateEntity({ id: args.characterId, conditions: merged } as any);
        changes.conditions = merged;
        resultParts.push(`conditions updated: ${JSON.stringify(merged)}`);
      }

      if (args.carriedObjects) {
        const scene = getSceneState();
        const { add, remove } = args.carriedObjects;
        if (add) {
          for (const objId of add) {
            scene.objectPositions[objId] = { type: "character", characterId: args.characterId };
          }
        }
        if (remove) {
          for (const objId of remove) {
            const pos = scene.objectPositions[objId];
            if (pos && pos.type === "character" && pos.characterId === args.characterId) {
              delete scene.objectPositions[objId];
            }
          }
        }
        setSceneState(scene);
        events.emitSceneUpdate(scene);
        if (add) resultParts.push(`objects given: [${add.join(", ")}]`);
        if (remove) resultParts.push(`objects removed: [${remove.join(", ")}]`);
      }

      if (Object.keys(changes).length > 0) {
        events.emitWorldUpdate(args.characterId, changes);
      }

      if (resultParts.length === 0) {
        return `Character '${existing.displayName}' unchanged. No fields were specified.`;
      }
      return `Character '${existing.displayName}' updated:\n${resultParts.map((p) => `- ${p}`).join("\n")}`;
    }, TOOL_NAMES.UPDATE_CHARACTER_STATE),
  });
}
