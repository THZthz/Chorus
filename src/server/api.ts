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
import chalk from "chalk";
import type { Message } from "@/types/dialogue";
import type { Character } from "@/types/entities";
import {
  generateTurn,
  generateTurnBatch,
  getSystemPromptTemplate,
  setSystemPromptTemplate,
  DEFAULT_SYSTEM_PROMPT_TEMPLATE,
} from "@/server/llm/index";
import { getAllEntities, seedDatabase, upsertEntity } from "@/server/models/world";
import { getHistory, addMessage, clearHistory, setHistory } from "@/server/models/history";
import { getAllPlots, getPlotById, updatePlot } from "@/server/models/plot";
import { getGameTime, getSceneState } from "@/server/models/scene";
import {
  getLlmLogs,
  clearLlmLogs,
  getConsoleLogs,
  addConsoleLog,
  clearConsoleLogs,
} from "@/server/models/debug";
import {
  getStep,
  saveStep,
  getChildSteps,
  getBranchPath,
  deactivateSiblingBranches,
  setBranchActive,
  saveAlternative,
  getAlternatives,
  setCurrentAlternative,
  getRootStep,
  getAllSteps,
  getChildByOption,
  getTreeStats,
  getLatestLeafStep,
  updateStepSnapshot,
} from "@/server/models/dialogue";

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

apiRouter.patch("/plots/:id", (req, res) => {
  const existing = getPlotById(req.params.id);
  if (!existing) {
    res.status(404).json({ error: "Plot not found" });
    return;
  }
  const result = updatePlot(req.params.id, req.body);
  if ("error" in result) {
    res.status(400).json({ error: result.error });
    return;
  }
  res.json({ success: true });
});

// ── Scene ──

apiRouter.get("/scene", (_req, res) => {
  res.json({ gameTime: getGameTime(), scene: getSceneState() });
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

// ── Chat (streaming SSE) ──

apiRouter.post("/chat/stream", async (req, res) => {
  try {
    const { userInput, history, parentStepId, parentOptionId, playerCharacter } = req.body;
    console.log(
      `[chat/stream] userInput="${String(userInput).slice(0, 80)}" parentStepId=${parentStepId} parentOptionId=${parentOptionId} historyLen=${history?.length ?? 0}`,
    );

    // Save the player's last YOU message to history
    const lastMsg = history?.[history.length - 1];
    if (lastMsg && lastMsg.type === "YOU") {
      try {
        addMessage(lastMsg);
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : String(err);
        if (!message.includes("UNIQUE constraint failed")) {
          // Non-fatal
        }
      }
    }

    await generateTurn(
      userInput,
      history ?? [],
      res,
      parentStepId ?? null,
      parentOptionId ?? null,
      playerCharacter ?? null,
    );
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
    const { stepId, history, playerCharacter } = req.body;

    if (!stepId) {
      res.status(400).json({ error: "stepId required" });
      return;
    }

    const step = getStep(stepId);
    if (!step) {
      res.status(404).json({ error: "Step not found" });
      return;
    }

    console.log(
      `[regenerate] archiving step=${stepId} parentStepId=${step.parentStepId} parentOptionId=${step.parentOptionId} historyLen=${history?.length ?? 0}`,
    );

    // Save current as alternative before regenerating
    const altId = saveAlternative(stepId, step.messages, step.options);
    console.log(`[regenerate] saved alternative ${altId} for step ${stepId}`);

    const lastYouMsg = history
      ? history.filter((m: { type: string }) => m.type === "YOU").pop()
      : null;
    const userInput = lastYouMsg?.text ?? "Continue";

    await generateTurn(
      `[REGENERATE] ${userInput}`,
      history ?? [],
      res,
      step.parentStepId ?? null,
      step.parentOptionId ?? null,
      playerCharacter ?? null,
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

// ── Tree replay (must be before /dialogue/:id to avoid route shadowing) ──

apiRouter.get("/dialogue/tree", (_req, res) => {
  const root = getRootStep();
  const allSteps = getAllSteps();
  const steps: Record<string, (typeof allSteps)[number]> = {};
  for (const step of allSteps) {
    steps[step.id] = step;
  }
  const stats = getTreeStats();
  console.log(
    `[dialogue/tree] root=${root?.id}, totalSteps=${allSteps.length}, leaves=${stats.leafIds.length}, branches=${stats.branchCount}`,
  );
  res.json({ root, steps, leafIds: stats.leafIds, stats });
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

// ── Session current ──

apiRouter.get("/session/current", (_req, res) => {
  const step = getLatestLeafStep();
  res.json(step ?? null);
});

apiRouter.patch("/dialogue/:id", (req, res) => {
  const step = getStep(req.params.id);
  if (!step) {
    res.status(404).json({ error: "Step not found" });
    return;
  }
  try {
    saveStep({ ...step, messages: req.body.messages, options: req.body.options });
    res.json({ success: true });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

apiRouter.patch("/dialogue/:id/snapshot", (req, res) => {
  const { worldSnapshot } = req.body;
  if (!worldSnapshot || typeof worldSnapshot !== "object") {
    res.status(400).json({ error: "worldSnapshot object required" });
    return;
  }
  const ok = updateStepSnapshot(req.params.id, worldSnapshot);
  if (!ok) {
    res.status(404).json({ error: "Step not found" });
    return;
  }
  res.json({ success: true });
});

apiRouter.post("/dialogue/traverse", (req, res) => {
  const { stepId, optionId } = req.body;
  if (!stepId || !optionId) {
    res.status(400).json({ error: "stepId and optionId required" });
    return;
  }

  const step = getStep(stepId);
  if (step) {
    const option = step.options.find((o) => o.id === optionId);
    if (option?.nextStepId) {
      const child = getStep(option.nextStepId);
      console.log(
        `[dialogue/traverse] fast-path: stepId=${stepId} optionId=${optionId} nextStepId=${option.nextStepId} found=${!!child}`,
      );
      res.json({ child: child || null });
      return;
    }
  }

  const child = getChildByOption(stepId, optionId);
  console.log(
    `[dialogue/traverse] fallback: stepId=${stepId} optionId=${optionId} found=${!!child}`,
  );
  res.json({ child: child || null });
});

// ── ID Generation ──

import { nextIdBatch } from "@/server/models/ids";

apiRouter.get("/ids/batch", (req, res) => {
  const count = Math.min(Math.max(parseInt(String(req.query.count), 10) || 20, 1), 100);
  res.json({ ids: nextIdBatch(count) });
});

const levelColor = (level: string) => {
  switch (level) {
    case "error": return chalk.red;
    case "warn":  return chalk.yellow;
    case "info":  return chalk.cyan;
    case "trace": return chalk.gray;
    default:      return chalk.white;
  }
};

const printClientLog = (level: string, message: string) => {
  const now = new Date().toLocaleTimeString([], { hour12: false });
  const color = levelColor(level);
  const tag = chalk.dim(`[${now}]`) + " " + chalk.magenta("[CLIENT]") + " " + color(`[${level.toUpperCase()}]`);
  process.stdout.write(`${tag} ${message}\n`);
};

// ── Debug ──

apiRouter.get("/debug/logs", (_req, res) => {
  res.json(getLlmLogs());
});

apiRouter.post("/debug/logs/clear", (_req, res) => {
  clearLlmLogs();
  res.json({ success: true });
});

apiRouter.get("/debug/console", (req, res) => {
  const limit = req.query.limit ? parseInt(req.query.limit as string, 10) : undefined;
  res.json(getConsoleLogs(limit));
});

apiRouter.post("/debug/console", (req, res) => {
  try {
    const body = req.body;
    const entries = Array.isArray(body) ? body : [body];
    for (const entry of entries) {
      const { level, message, args } = entry;
      addConsoleLog(level, message, args);
      printClientLog(level, message);
    }
    res.json({ success: true });
  } catch (error: unknown) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: errorMessage });
  }
});

apiRouter.post("/debug/console/clear", (_req, res) => {
  clearConsoleLogs();
  res.json({ success: true });
});

// ── System Prompt ──

apiRouter.get("/debug/system-prompt", (_req, res) => {
  res.json({ template: getSystemPromptTemplate() });
});

apiRouter.put("/debug/system-prompt", (req, res) => {
  try {
    const { template } = req.body;
    if (typeof template !== "string" || !template.trim()) {
      res.status(400).json({ error: "template string required" });
      return;
    }
    setSystemPromptTemplate(template);
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
    import("./db").then(({ default: db }) => {
      db.prepare("DELETE FROM system_state WHERE key = ?").run("gm_system_prompt");
      res.json({ success: true });
    });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    res.status(500).json({ error: message });
  }
});

// ── Reset ──

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
