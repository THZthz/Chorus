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
import { getGameTime, getSceneState, describeTime } from "@/server/models/scene";
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({});

export function createGetSceneTool() {
  return tool({
    title: "Get Scene",
    description:
      "Returns the current game time and full scene state: where each character is, where each object is (and who is carrying it). Use this to check the current situation before making changes.",
    inputSchema,
    execute: wrapSafe(async (_args: z.infer<typeof inputSchema>) => {
      const time = getGameTime();
      const scene = getSceneState();

      const lines: string[] = [];
      lines.push("## Current Scene");
      lines.push("");
      lines.push(`**Game Time:** ${describeTime(time)}`);
      lines.push("");

      const charsHere = Object.entries(scene.characterLocations).filter(
        ([, locId]) => locId === scene.currentLocationId,
      );
      const charsElsewhere = Object.entries(scene.characterLocations).filter(
        ([, locId]) => locId !== scene.currentLocationId,
      );

      lines.push("### Characters Present");
      lines.push("");
      if (charsHere.length > 0) {
        lines.push("| Character | Location |");
        lines.push("|---|---|");
        for (const [charId, locId] of [...charsHere, ...charsElsewhere]) {
          lines.push(`| \`${charId}\` | \`${locId}\` |`);
        }
      } else {
        lines.push("*No characters in scene*");
      }
      lines.push("");

      lines.push("### Object Positions");
      lines.push("");
      if (Object.keys(scene.objectPositions).length > 0) {
        lines.push("| Object | Position |");
        lines.push("|---|---|");
        for (const [objId, pos] of Object.entries(scene.objectPositions)) {
          const where =
            pos.type === "character"
              ? `Carried by \`${pos.characterId}\``
              : `At \`${pos.locationId}\``;
          lines.push(`| \`${objId}\` | ${where} |`);
        }
      } else {
        lines.push("*No objects in scene*");
      }

      return lines.join("\n").trim();
    }, TOOL_NAMES.GET_SCENE),
  });
}
