/**
 * Chorus — cinematic RPG-style dialogue engine
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
import { wrapSafe } from "@/server/llm/tools/shared";
import { getObserver } from "@/server/llm/sceneObserver";
import { TOOL_NAMES } from "@/shared/constants";

export const resetSceneContext = tool({
  title: TOOL_NAMES.RESET_SCENE_CONTEXT,
  description:
    "Reset the scene context observer so the next turn shows full entity and plot descriptions instead of briefs. Use when the scene has significantly changed and you want the player to see full descriptions again.",
  inputSchema: z.object({}),
  execute: wrapSafe(async () => {
    getObserver().reset();
    return "Scene context observer reset. Next turn will show full descriptions for all entities and plots.";
  }, TOOL_NAMES.RESET_SCENE_CONTEXT),
});
