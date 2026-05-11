import { z } from "zod";

export const chatStreamSchema = z.object({
  userInput: z.string(),
  history: z.array(z.any()).optional().default([]),
});
