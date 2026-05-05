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
  GET_ALL_ENTITIES: "getAllEntitiesName",
  QUERY_ENTITY: "queryEntity",
  EDIT_ENTITY: "editEntity",
  CREATE_PLOT: "createPlot",
  EDIT_PLOT: "editPlot",
  GET_PLOT: "getPlot",
  GENERATE_DIALOGUE: "generateDialogueStep",
  ADVANCE_TIME: "advanceTime",
  UPDATE_SCENE: "updateScene",
  GET_SCENE: "getScene",
} as const;

export type ToolName = (typeof TOOL_NAMES)[keyof typeof TOOL_NAMES];
