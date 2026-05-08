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

export const SKILL_NAMES = [
  "LOGIC",
  "RHETORIC",
  "EMPATHY",
  "PERCEPTION",
  "VOLITION",
  "ENDURANCE",
  "SORCERY",
  "SUGGESTION",
  "INSTINCT",
  "MIGHT",
  "CLOCKWORK",
  "ALCHEMY",
] as const;

export type SkillName = (typeof SKILL_NAMES)[number];

export const TOOL_NAMES = {
  LIST_ENTITIES: "listEntities",
  GET_ENTITY: "getEntity",
  UPDATE_ENTITY: "updateEntity",
  UPDATE_ENTITIES: "updateEntities",
  CREATE_ENTITY: "createEntity",
  GET_CHARACTER_STATE: "getCharacterState",
  UPDATE_CHARACTER_STATE: "updateCharacterState",
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
