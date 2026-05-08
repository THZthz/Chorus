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
      return JSON.stringify(
        {
          gameTime: { day: time.day, segment: time.segment, label: describeTime(time) },
          scene,
        },
        null,
        2,
      );
    }, TOOL_NAMES.GET_SCENE),
  });
}
