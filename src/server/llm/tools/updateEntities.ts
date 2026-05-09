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
  entries: z
    .array(
      z.object({
        id: z.string().describe("Entity ID to update."),
        shortDescription: z.string().optional().describe("New concise label."),
        longDescription: z.string().optional().describe("New detailed observation."),
        attributes: z
          .record(z.string(), z.string())
          .optional()
          .describe("Physical or mental traits (merged)."),
        opinions: z
          .record(z.string(), z.string())
          .optional()
          .describe("Opinion changes (merged, CHARACTER only)."),
      }),
    )
    .describe("Array of entity updates to apply."),
});

export function createUpdateEntitiesTool(events: EventEmitter) {
  return tool({
    title: "Update Entities",
    description:
      "Bulk-update multiple entities at once. Each entry needs an id and any combination of shortDescription, longDescription, attributes, or opinions. Emits one world_update per entity.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const results: string[] = [];
      for (const entry of args.entries) {
        const existing = getEntityById(entry.id);
        if (!existing) {
          results.push(`ERROR: Entity '${entry.id}' not found — skipped.`);
          continue;
        }
        updateEntity(entry);
        const changes: Record<string, unknown> = {};
        if (entry.longDescription != null) changes.longDescription = entry.longDescription;
        if (entry.shortDescription != null) changes.shortDescription = entry.shortDescription;
        if (entry.attributes) changes.attributes = entry.attributes;
        if (entry.opinions) changes.opinions = entry.opinions;
        events.emitWorldUpdate(entry.id, changes);
        results.push(`Updated '${existing.displayName}' (${entry.id}).`);
      }
      return results.join("\n");
    }, TOOL_NAMES.UPDATE_ENTITIES),
  });
}
