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

export const SEGMENT_LABELS: Record<number, string> = {
  0: "Midnight",
  1: "Late Night",
  2: "Dawn",
  3: "Early Morning",
  4: "Morning",
  5: "Late Morning",
  6: "Noon",
  7: "Afternoon",
  8: "Late Afternoon",
  9: "Dusk",
  10: "Evening",
  11: "Night",
};

export const SEGMENT_HOURS: Record<number, string> = {
  0: "12am–2am",
  1: "2am–4am",
  2: "4am–6am",
  3: "6am–8am",
  4: "8am–10am",
  5: "10am–12pm",
  6: "12pm–2pm",
  7: "2pm–4pm",
  8: "4pm–6pm",
  9: "6pm–8pm",
  10: "8pm–10pm",
  11: "10pm–12am",
};

export const TOOL_NAMES = {
  QUERY_WORLD: "queryWorld",
  MUTATE_WORLD: "mutateWorld",
  GENERATE_DIALOGUE: "generateDialogueStep",
  ADVANCE_TIME: "advanceTime",
  SEARCH_MEMORY: "searchWorld",
  EDIT_NOTE: "editNote",
  SEARCH_NOTES: "searchNotes",
  EDIT_PLOT: "editPlot",
  SEARCH_PLOTS: "searchPlots",
  RESET_SCENE_CONTEXT: "resetSceneContext",
  MANAGE_SCHEMA: "manageSchema",
} as const;
