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

import express from "express";
import { generateTurn } from "@/server/llm";
import { chatStreamSchema } from "@/server/validation";

const apiRouter = express.Router();

// ── Chat (streaming SSE) ──

apiRouter.post("/chat/stream", async (req, res) => {
  const parsed = chatStreamSchema.safeParse(req.body);
  if (!parsed.success) {
    res.status(400).json({ error: "Invalid request", details: parsed.error.issues });
    return;
  }
  try {
    const { userInput, history } = parsed.data;
    console.log(
      `[chat/stream] userInput="${String(userInput).slice(0, 80)}" historyLen=${history?.length ?? 0}`,
    );
    await generateTurn(userInput, history ?? [], res);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("Chat stream error:", message);
    if (!res.headersSent) {
      res.status(500).json({ error: message });
    } else {
      res.write(`event: error\ndata: ${JSON.stringify({ message })}\n\n`);
      res.end();
    }
  }
});

// ── History ──

apiRouter.get("/history", (_req, res) => {
  // Return empty for now — the GM uses memory_get_conversation for history.
  // Console client uses this for resume; returns empty array as placeholder.
  res.json([]);
});

// ── Session current ──

apiRouter.get("/session/current", (_req, res) => {
  // No dialogue tree — return null to signal fresh session
  res.json(null);
});

// ── Reset ──

apiRouter.post("/reset", async (_req, res) => {
  try {
    // Clear Neo4j and re-seed
    const { clearNeo4jDatabase } = await import("@/server/mcp/reset");
    await clearNeo4jDatabase();
    const { seedDatabase } = await import("@/server/mcp/seed");
    await seedDatabase();
    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

export default apiRouter;
