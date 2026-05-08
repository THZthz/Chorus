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

export const TOOL_NAMES = {
  LIST_ENTITIES: "listEntities",
  GET_ENTITY: "getEntity",
  UPDATE_ENTITY: "updateEntity",
  CREATE_PLOT: "createPlot",
  UPDATE_PLOT: "updatePlot",
  GET_PLOT: "getPlot",
  GENERATE_DIALOGUE: "generateDialogue",
  ADVANCE_TIME: "advanceTime",
  UPDATE_SCENE: "updateScene",
  GET_SCENE: "getScene",
  ADD_FACT: "addFact",
  GET_FACT: "getFact",
  UPDATE_FACT: "updateFact",
  REMOVE_FACT: "removeFact",
} as const;

export type ToolName = (typeof TOOL_NAMES)[keyof typeof TOOL_NAMES];
