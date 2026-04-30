import express from "express";
import { generateAIResponse, generateStreamingResponse } from "@/server/LlmServiceBackend";
import { getAllEntities, seedDatabase, upsertEntity } from "@/server/models/world";
import { getHistory, addMessage, clearHistory, setHistory } from "@/server/models/history";
import { getAllPlots } from "@/server/models/plot";
import { getLlmLogs, clearLlmLogs, getConsoleLogs, addConsoleLog, clearConsoleLogs } from "@/server/models/debug";
import { getStep, getChildSteps, getBranchPath, deactivateSiblingBranches, setBranchActive, saveAlternative, getAlternatives, setCurrentAlternative } from "@/server/models/dialogue";

const apiRouter = express.Router();

seedDatabase();

// ── World ──

apiRouter.get("/world", (_req, res) => {
  res.json(getAllEntities());
});

apiRouter.post("/world/entity", (req, res) => {
  try {
    upsertEntity(req.body);
    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

// ── Plots ──

apiRouter.get("/plots", (_req, res) => {
  res.json(getAllPlots());
});

// ── History ──

apiRouter.get("/history", (_req, res) => {
  res.json(getHistory());
});

apiRouter.post("/history", (req, res) => {
  try {
    setHistory(req.body);
    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

// ── Dialogue Tree ──

apiRouter.get("/dialogue/:id", (req, res) => {
  const step = getStep(req.params.id);
  if (!step) {
    res.status(404).json({ error: "Step not found" });
    return;
  }
  const children = getChildSteps(req.params.id);
  const alternatives = getAlternatives(req.params.id);
  res.json({ step, children, alternatives });
});

apiRouter.get("/dialogue/:id/path", (req, res) => {
  const path = getBranchPath(req.params.id);
  res.json(path);
});

apiRouter.get("/dialogue/:id/children", (req, res) => {
  const children = getChildSteps(req.params.id);
  res.json(children);
});

// ── Chat (legacy, non-streaming) ──

apiRouter.post("/chat", async (req, res) => {
  try {
    const { userInput, history } = req.body;

    const lastMsg = history[history.length - 1];
    if (lastMsg && lastMsg.type === "YOU") {
      try {
        addMessage(lastMsg);
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : String(err);
        if (!message.includes("UNIQUE constraint failed")) {
          throw err;
        }
      }
    }

    const rawResponse = await generateAIResponse(userInput, history);
    res.json(rawResponse);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("Chat error:", message);
    res.status(500).json({ error: message });
  }
});

// ── Chat (streaming SSE) ──

apiRouter.post("/chat/stream", async (req, res) => {
  try {
    const { userInput, history, parentStepId, parentOptionId } = req.body;

    // Save player message
    const lastMsg = history[history.length - 1];
    if (lastMsg && lastMsg.type === "YOU") {
      try {
        addMessage(lastMsg);
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : String(err);
        if (!message.includes("UNIQUE constraint failed")) {
          // Non-fatal for player messages
        }
      }
    }

    await generateStreamingResponse(
      userInput,
      history,
      res,
      parentStepId ?? null,
      parentOptionId ?? null
    );
    // Response is handled inside generateStreamingResponse (SSE)
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

// ── Regenerate ──

apiRouter.post("/regenerate", async (req, res) => {
  try {
    const { stepId, parentStepId, parentOptionId, history } = req.body;

    if (!stepId) {
      res.status(400).json({ error: "stepId required" });
      return;
    }

    const step = getStep(stepId);
    if (!step) {
      res.status(404).json({ error: "Step not found" });
      return;
    }

    // Save current as alternative
    saveAlternative(stepId, step.messages, step.options);

    // Generate new response via SSE
    const lastUserMsg = history
      ? history.filter((m: { type: string }) => m.type === "YOU").pop()
      : null;
    const userInput = lastUserMsg?.text ?? "Continue";

    await generateStreamingResponse(
      `[REGENERATE] ${userInput}`,
      history ?? [],
      res,
      parentStepId ?? null,
      parentOptionId ?? null
    );
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("Regenerate error:", message);
    if (!res.headersSent) {
      res.status(500).json({ error: message });
    } else {
      res.write(`event: error\ndata: ${JSON.stringify({ message })}\n\n`);
      res.end();
    }
  }
});

// ── Alternatives ──

apiRouter.get("/dialogue/:id/alternatives", (req, res) => {
  const alternatives = getAlternatives(req.params.id);
  res.json(alternatives);
});

apiRouter.post("/dialogue/:id/alternatives/:altId/select", (req, res) => {
  try {
    setCurrentAlternative(req.params.id, req.params.altId);
    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

// ── Branch Management ──

apiRouter.post("/branches/activate", (req, res) => {
  try {
    const { stepId, parentStepId } = req.body;
    setBranchActive(stepId, true);
    if (parentStepId) {
      deactivateSiblingBranches(parentStepId, stepId);
    }
    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

// ── Debug ──

apiRouter.get("/debug/logs", (_req, res) => {
  res.json(getLlmLogs());
});

apiRouter.post("/debug/logs/clear", (_req, res) => {
  clearLlmLogs();
  res.json({ success: true });
});

apiRouter.get("/debug/console", (_req, res) => {
  res.json(getConsoleLogs());
});

apiRouter.post("/debug/console", (req, res) => {
  try {
    const { level, message, args } = req.body;
    addConsoleLog(level, message, args);
    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

apiRouter.post("/debug/console/clear", (_req, res) => {
  clearConsoleLogs();
  res.json({ success: true });
});

apiRouter.post("/reset", (_req, res) => {
  import("./db").then(({ default: db }) => {
    db.prepare("DELETE FROM history_messages").run();
    db.prepare("DELETE FROM plots").run();
    db.prepare("DELETE FROM entities").run();
    db.prepare("DELETE FROM dialogue_steps").run();
    db.prepare("DELETE FROM dialogue_alternatives").run();
    seedDatabase();
    res.json({ success: true });
  });
});

export default apiRouter;
