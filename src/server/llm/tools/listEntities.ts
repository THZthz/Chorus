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
import { getAllEntitySummaries } from "@/server/models/world";
import { ENTITY_TYPES } from "@/types/entities";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  type: z.enum(ENTITY_TYPES).optional().describe("Optional filter by entity type."),
});

export function createListEntitiesTool() {
  return tool({
    title: "List Entities",
    description: "Returns the id, displayName, type, and shortDescription of all world entities.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const summaries = getAllEntitySummaries(args.type);
      if (summaries.length === 0) return "No entities found.";

      const byType: Record<string, typeof summaries> = {};
      for (const e of summaries) {
        (byType[e.type] ??= []).push(e);
      }

      const lines: string[] = [`## Entities (${summaries.length} total)`, ""];
      const order = ["CHARACTER", "LOCATION", "OBJECT"];
      for (const type of order) {
        const group = byType[type];
        if (!group || group.length === 0) continue;
        lines.push(`### ${type}S (${group.length})`);
        lines.push("");
        for (const e of group) {
          lines.push(`- **\`${e.id}\`** — ${e.displayName} — ${e.shortDescription}`);
        }
        lines.push("");
      }
      return lines.join("\n").trim();
    }, TOOL_NAMES.LIST_ENTITIES),
  });
}
