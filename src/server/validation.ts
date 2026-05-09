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

import { z } from "zod";

export const chatStreamSchema = z.object({
  userInput: z.string(),
  history: z.array(z.any()).optional().default([]),
  parentStepId: z.string().nullable().optional().default(null),
  parentOptionId: z.string().nullable().optional().default(null),
  playerCharacter: z.any().nullable().optional().default(null),
});

export const upsertEntitySchema = z
  .object({
    id: z.string(),
    type: z.string(),
    displayName: z.string(),
  })
  .passthrough();

export const patchDialogueSchema = z.object({
  messages: z.array(z.any()),
  options: z.array(z.any()),
});

export const patchPlotSchema = z.object({
  title: z.string().optional(),
  status: z.string().optional(),
  description: z.string().optional(),
  involvedLocations: z.array(z.string()).optional(),
  involvedCharacters: z.array(z.string()).optional(),
  childPlots: z.array(z.any()).optional(),
  flags: z.record(z.string(), z.any()).optional(),
  addChildPlot: z.any().optional(),
  removeChildPlot: z.number().optional(),
});

export const patchSnapshotSchema = z.object({
  worldSnapshot: z.record(z.string(), z.any()),
});

export const systemPromptSchema = z.object({
  template: z.string().min(1),
});

export const regenerateSchema = z.object({
  stepId: z.string(),
  history: z.array(z.any()).optional().default([]),
  playerCharacter: z.any().nullable().optional().default(null),
});

export const traverseSchema = z.object({
  stepId: z.string(),
  optionId: z.string(),
});

export const activateBranchSchema = z.object({
  stepId: z.string(),
  parentStepId: z.string().optional(),
});

export const pregenSchema = z.object({
  size: z.number().min(2).max(50).optional().default(10),
});
