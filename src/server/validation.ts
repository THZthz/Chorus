import { z } from "zod";

export const chatStreamSchema = z.object({
  userInput: z.string(),
  history: z.array(z.any()).optional().default([]),
  parentStepId: z.string().nullable().optional().default(null),
  parentOptionId: z.string().nullable().optional().default(null),
  playerCharacter: z.any().nullable().optional().default(null),
});

export const upsertEntitySchema = z.object({
  id: z.string(),
  type: z.string(),
  displayName: z.string(),
}).passthrough();

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
