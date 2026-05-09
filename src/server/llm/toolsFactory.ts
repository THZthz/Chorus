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

import type { EventEmitter } from "@/server/llm/events";
import { TOOL_NAMES } from "@/shared/constants";
import {
  createListEntitiesTool,
  createGetEntityTool,
  createUpdateEntityTool,
  createUpdateEntitiesTool,
  createCreateEntityTool,
  createGetCharacterStateTool,
  createUpdateCharacterStateTool,
  createCreatePlotTool,
  createUpdatePlotTool,
  createGetPlotTool,
  createGenerateDialogueStepTool,
  createAdvanceTimeTool,
  createUpdateSceneTool,
  createGetSceneTool,
  createAddFactTool,
  createGetFactTool,
  createUpdateFactTool,
  createRemoveFactTool,
} from "@/server/llm/tools";

export function createAllTools(
  events: EventEmitter,
  dialogueTool: ReturnType<typeof createGenerateDialogueStepTool>["tool"],
) {
  return {
    listEntities: createListEntitiesTool(),
    getEntity: createGetEntityTool(),
    updateEntity: createUpdateEntityTool(events),
    updateEntities: createUpdateEntitiesTool(events),
    createEntity: createCreateEntityTool(events),
    getCharacterState: createGetCharacterStateTool(),
    updateCharacterState: createUpdateCharacterStateTool(events),
    createPlot: createCreatePlotTool(events),
    updatePlot: createUpdatePlotTool(events),
    getPlot: createGetPlotTool(),
    getScene: createGetSceneTool(),
    updateScene: createUpdateSceneTool(events),
    advanceTime: createAdvanceTimeTool(events),
    addFact: createAddFactTool(events),
    getFact: createGetFactTool(),
    updateFact: createUpdateFactTool(events),
    removeFact: createRemoveFactTool(events),
    generateDialogueStep: dialogueTool,
  };
}
