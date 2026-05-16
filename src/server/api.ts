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

import express from "express";
import { generateTurn } from "@/server/llm";
import { chatStreamSchema } from "@/server/validation";
import { MemoryClient } from "@/server/memory/client";
import { RelationshipManager } from "@/server/memory/relationshipManager";
import { getCurrentOptions } from "@/server/memory/gameState";
import { buildFullSceneContext } from "@/server/llm/sceneContext";
import type { Message } from "@/types/dialogue";

const apiRouter = express.Router();

// ── Chat (streaming SSE) ──

apiRouter.post("/chat/stream", async (req, res) => {
  const parsed = chatStreamSchema.safeParse(req.body);
  if (!parsed.success) {
    res.status(400).json({ error: "Invalid request", details: parsed.error.issues });
    return;
  }
  try {
    const { userInput, history, check } = parsed.data;
    console.log(
      `[chat/stream] userInput="${String(userInput).slice(0, 80)}" historyLen=${history?.length ?? 0} hasCheck=${!!check}`,
    );
    await generateTurn(userInput, history ?? [], res, check);
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

apiRouter.get("/history", async (_req, res) => {
  try {
    const client = MemoryClient.getCachedInstance();
    const messages = await client.shortTerm.getConversation();
    const history: Message[] = messages.map((m) => {
      const meta = m.metadata || {};
      const isPlayer = m.role === "user";
      const msg: Message = {
        id: m.id,
        speaker: isPlayer ? "YOU" : (meta.speaker as string) || "SYSTEM",
        type: isPlayer ? "YOU" : (meta.type as Message["type"]) || "SYSTEM",
        text: m.content || "",
        metadata: isPlayer ? undefined : (meta as Message["metadata"]),
      };
      if (meta.rollResult) {
        msg.rollResult = meta.rollResult as Message["rollResult"];
      }
      return msg;
    });
    res.json(history);
  } catch (error: unknown) {
    console.error("History fetch error:", error);
    res.json([]);
  }
});

// ── Game state ──

apiRouter.get("/game/current", async (_req, res) => {
  try {
    const state = await getCurrentOptions();
    res.json(state);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("Session state fetch error:", message);
    res.json(null);
  }
});

// ── Dump (developer debug: full world state) ──

apiRouter.get("/dump", async (_req, res) => {
  try {
    const md = await buildFullSceneContext();
    res.set("Content-Type", "text/plain; charset=utf-8").send(md);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("Dump fetch error:", message);
    res.status(500).json({ error: message });
  }
});

// ── Reset ──

apiRouter.post("/reset", async (_req, res) => {
  try {
    // Clear Neo4j and re-seed
    const { clearNeo4jDatabase } = await import("@/server/memory/reset");
    await clearNeo4jDatabase();
    const { seedDatabase } = await import("@/server/seed-stories/seed");
    await seedDatabase();

    // Reset in-memory GM_DEFINED types, then sync INTERNAL + PREDEFINED back to Neo4j
    const relManager = RelationshipManager.getCachedInstance();
    relManager.reset();
    const nodeManager = (
      await import("@/server/memory/nodeManager")
    ).NodeManager.getCachedInstance();
    nodeManager.reset();
    const client = await MemoryClient.getInstance();
    await relManager.syncToNeo4j(client.neo4j);
    await nodeManager.syncToNeo4j(client.neo4j);

    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

export default apiRouter;
