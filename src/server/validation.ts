import { z } from "zod";

export const chatStreamSchema = z.object({
  userInput: z.string(),
  history: z.array(z.any()).optional().default([]),
});

export const systemPromptSchema = z.object({
  template: z.string().min(1),
});
