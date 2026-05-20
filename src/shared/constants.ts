/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias
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
  QUERY_WORLD: "queryWorld",
  GENERATE_DIALOGUE: "generateDialogueStep",
  ADVANCE_TIME: "advanceTime",
  SEARCH_WORLD: "searchWorld",
  MANAGE_SCHEMA: "manageSchema",
  EDIT_NODE: "editNode",
  EDIT_RELATIONSHIP: "editRelationship",
  EDIT_NOTE: "editNote",
  EDIT_PLOT: "editPlot",
  GET_CONTEXT: "getContext",
} as const;
