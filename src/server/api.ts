import express from "express";
import { generateTurn, getSystemPromptTemplate, setSystemPromptTemplate, DEFAULT_SYSTEM_PROMPT_TEMPLATE } from "@/server/llm";
import { chatStreamSchema, systemPromptSchema } from "@/server/validation";
import { getLlmLogs, clearLlmLogs } from "@/server/models/debug";
import { nextId, nextIdBatch } from "@/server/models/ids";
import db from "@/server/db";

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
    console.log(`[chat/stream] userInput="${String(userInput).slice(0, 80)}" historyLen=${history?.length ?? 0}`);
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

// ── ID Generation ──

apiRouter.get("/ids/batch", (req, res) => {
  const count = Math.min(Math.max(parseInt(String(req.query.count), 10) || 20, 1), 100);
  res.json({ ids: nextIdBatch(count) });
});

// ── Debug ──

apiRouter.get("/debug/logs", (_req, res) => {
  res.json(getLlmLogs());
});

apiRouter.post("/debug/logs/clear", (_req, res) => {
  clearLlmLogs();
  res.json({ success: true });
});

// ── System Prompt ──

apiRouter.get("/debug/system-prompt", (_req, res) => {
  res.json({ template: getSystemPromptTemplate() });
});

apiRouter.put("/debug/system-prompt", (req, res) => {
  const parsed = systemPromptSchema.safeParse(req.body);
  if (!parsed.success) {
    res.status(400).json({ error: "Invalid request", details: parsed.error.issues });
    return;
  }
  try {
    setSystemPromptTemplate(parsed.data.template);
    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

apiRouter.get("/debug/system-prompt/default", (_req, res) => {
  res.json({ template: DEFAULT_SYSTEM_PROMPT_TEMPLATE });
});

apiRouter.post("/debug/system-prompt/reset", (_req, res) => {
  try {
    db.prepare("DELETE FROM system_state WHERE key = ?").run("gm_system_prompt");
    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

// ── Reset ──

apiRouter.post("/reset", async (_req, res) => {
  try {
    // Clear SQLite logs
    db.prepare("DELETE FROM llm_logs").run();
    db.prepare("DELETE FROM llm_steps").run();
    // Re-seed time to initial state
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run("game_time_day", "1");
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run("game_time_segment", "2");
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
