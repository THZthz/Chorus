/**
 * Chorus — cinematic dialogue engine
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
import {
  buildSceneContext,
  buildCharactersBrief,
  buildLocationsBrief,
  buildObjectsBrief,
  buildPlotsBrief,
  buildRelationshipDump,
} from "@/server/llm/sceneContext";
import {
  getSchemaVisualization,
  getRelationshipTypeDescriptions,
  formatSchemaMarkdown,
} from "@/server/models/schema";
import { stripHiddenProperties } from "@/server/memory/neo4j";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { searchWorld } from "@/server/llm/tools/searchWorld";
import { editNode } from "@/server/llm/tools/editNode";
import { editRelationship } from "@/server/llm/tools/editRelationship";
import { manageSchema } from "@/server/llm/tools/manageSchema";
import { resetSceneContext } from "@/server/llm/tools/resetSceneContext";
import type { Message } from "@/types/dialogue";

const debugToolRegistry: Record<string, { execute: (args: any) => Promise<string> }> = {
  queryWorld: queryWorld as any,
  searchWorld: searchWorld as any,
  editNode: editNode as any,
  editRelationship: editRelationship as any,
  manageSchema: manageSchema as any,
  resetSceneContext: resetSceneContext as any,
};

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
    const history: Message[] = messages.map((m, i) => {
      const meta = m.metadata || {};
      const msg: Message = {
        id: `msg_${i}`,
        speaker: (meta.speaker as string) || "SYSTEM",
        type: (meta.type as Message["type"]) || "SYSTEM",
        text: m.content || "",
        metadata: meta as Message["metadata"],
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

apiRouter.get("/debug/dump", async (_req, res) => {
  try {
    const db = MemoryClient.getCachedInstance().neo4j;
    const [scene, characters, locations, objects, plots, relationships, schemaVis, relTypeDescs] =
      await Promise.all([
        buildSceneContext(),
        buildCharactersBrief(),
        buildLocationsBrief(),
        buildObjectsBrief(),
        buildPlotsBrief(),
        buildRelationshipDump(),
        getSchemaVisualization(db),
        getRelationshipTypeDescriptions(db),
      ]);
    const schema = formatSchemaMarkdown(schemaVis, relTypeDescs);
    const md = [scene, characters, locations, objects, plots, relationships, schema].join("\n");
    res.set("Content-Type", "text/plain; charset=utf-8").send(md);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("Dump fetch error:", message);
    res.status(500).json({ error: message });
  }
});

// ── Debug search endpoints ──

apiRouter.get("/debug/search/world", async (req, res) => {
  try {
    const query = (req.query.query as string) || "";
    if (!query) {
      res.status(400).json({ error: "Missing ?query parameter" });
      return;
    }
    const types = (req.query.types as string)?.split(",").filter(Boolean) ?? [
      "entities",
      "messages",
    ];
    const limit = parseInt(req.query.limit as string, 10) || 10;
    const threshold = parseFloat(req.query.threshold as string) || undefined;
    const rerank = req.query.rerank === "true";
    const client = MemoryClient.getCachedInstance();
    const results = await client.search.search(query, {
      memoryTypes: types,
      limit,
      threshold: threshold || undefined,
      rerank,
    });
    res.json(stripHiddenProperties(results));
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("Debug search/world error:", message);
    res.status(500).json({ error: message });
  }
});

apiRouter.get("/debug/search/plots", async (req, res) => {
  try {
    const query = (req.query.query as string) || "";
    if (!query) {
      res.status(400).json({ error: "Missing ?query parameter" });
      return;
    }
    const limit = parseInt(req.query.limit as string, 10) || 10;
    const threshold = parseFloat(req.query.threshold as string) || undefined;
    const rerank = req.query.rerank === "true";
    const client = MemoryClient.getCachedInstance();
    const results = await client.plots.searchPlots(query, {
      limit,
      threshold: threshold || undefined,
      rerank,
    });
    res.json(stripHiddenProperties(results));
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("Debug search/plots error:", message);
    res.status(500).json({ error: message });
  }
});

apiRouter.get("/debug/search/notes", async (req, res) => {
  try {
    const query = (req.query.query as string) || "";
    if (!query) {
      res.status(400).json({ error: "Missing ?query parameter" });
      return;
    }
    const limit = parseInt(req.query.limit as string, 10) || 10;
    const threshold = parseFloat(req.query.threshold as string) || undefined;
    const rerank = req.query.rerank === "true";
    const client = MemoryClient.getCachedInstance();
    const results = await client.notes.searchNotes(query, {
      limit,
      threshold: threshold || undefined,
      rerank,
    });
    res.json(stripHiddenProperties(results));
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("Debug search/notes error:", message);
    res.status(500).json({ error: message });
  }
});

// ── Debug tool invocation ──

apiRouter.post("/debug/tools/:toolName", async (req, res) => {
  const tool = debugToolRegistry[req.params.toolName];
  if (!tool) {
    res.status(404).json({ error: `Unknown tool: ${req.params.toolName}` });
    return;
  }
  try {
    const result = await tool.execute(req.body ?? {});
    res.set("Content-Type", "text/plain; charset=utf-8").send(result);
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
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
