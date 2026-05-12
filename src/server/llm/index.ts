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

import { streamText, stepCountIs } from "ai";
import { parse as parsePartial } from "partial-json";
import type { Response } from "express";
import type { Message, DialogueOption } from "@/types/dialogue";
import { TurnEventEmitter } from "@/server/llm/events";
import { buildSystemPrompt, MAX_GM_STEPS } from "@/server/llm/prompt";
import { getModel } from "@/server/llm/model";
import { MemoryClient } from "@/server/memory/client";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { mutateWorld } from "@/server/llm/tools/mutateWorld";
import { searchMemory } from "@/server/llm/tools/searchMemory";
import { editNote } from "@/server/llm/tools/editNote";
import { searchNotes } from "@/server/llm/tools/searchNotes";
import { editPlot } from "@/server/llm/tools/editPlot";
import { searchPlots } from "@/server/llm/tools/searchPlots";
import { saveCurrentOptions } from "@/server/memory/gameState";
import { createGenerateDialogueStepTool } from "@/server/llm/tools/generateDialogueStep";
import { createAdvanceTimeTool } from "@/server/llm/tools/advanceTime";
import { type SkillName } from "@/shared/constants";
export { DEFAULT_SYSTEM_PROMPT_TEMPLATE, buildSystemPrompt } from "@/server/llm/prompt";

export async function generateTurn(
  userInput: string,
  history: Message[],
  res: Response,
): Promise<void> {
  const systemPrompt = await buildSystemPrompt();
  const events = new TurnEventEmitter(res);

  console.log(
    `[generateTurn] historyLen=${history.length} userInput="${String(userInput).slice(0, 80)}"`,
  );

  res.writeHead(200, {
    "Content-Type": "text/event-stream",
    "Cache-Control": "no-cache",
    Connection: "keep-alive",
    "X-Accel-Buffering": "no",
  });

  events.startStep(`step_${Date.now()}`);

  // Persist player input so full conversation is available for resume
  {
    const client = MemoryClient.getCachedInstance();
    await client.shortTerm.addMessage("user", userInput);
  }

  const historyWindow = 10;
  const promptText = [
    `## DIALOGUE HISTORY (Last ${historyWindow})`,
    history
      .slice(-historyWindow)
      .map((m) => `${m.speaker} (${m.type}): ${m.text}`)
      .join("\n"),
    "",
    "---",
    "",
    "## PLAYER ACTION",
    `The player just said/did: "${userInput}"`,
    "",
    "Generate the narrative response following the output format exactly.",
  ].join("\n");

  const { model } = getModel();

  let finalMessages: Record<string, unknown>[] = [];
  let finalOptions: DialogueOption[] = [];

  // Auto-persist each generated message to Neo4j after validation passes.
  const persistMessage = async (msg: {
    speaker: string;
    type: string;
    text: string;
    metadata?: Record<string, unknown>;
  }) => {
    const client = MemoryClient.getCachedInstance();
    const role: "user" | "assistant" | "system" = msg.type === "CHARACTER" ? "assistant" : "system";
    const stored = await client.shortTerm.addMessage(role, msg.text, {
      speaker: msg.speaker,
      type: msg.type,
      ...msg.metadata,
    });
    await client.observer.onMessageStored(msg.text, stored.id, role);
  };

  const dialogueStepTool = createGenerateDialogueStepTool(events, persistMessage);
  const advanceTimeTool = createAdvanceTimeTool(events);

  const allTools = {
    queryWorld,
    mutateWorld,
    searchMemory,
    editNote,
    searchNotes,
    editPlot,
    searchPlots,
    generateDialogueStep: dialogueStepTool.tool,
    advanceTime: advanceTimeTool,
  };

  let streamError: string | null = null;

  try {
    const result = streamText({
      model,
      system: systemPrompt,
      messages: [{ role: "user", content: promptText }],
      tools: allTools,
      stopWhen: [
        (state) => {
          const called = state.steps.some((s) =>
            s.toolCalls?.some((tc) => tc.toolName === "generateDialogueStep"),
          );
          return called && dialogueStepTool.wasValid();
        },
        stepCountIs(MAX_GM_STEPS),
      ],
      prepareStep: (
        (nudgeState: { count: number; timeReminded: boolean }) =>
        ({ steps, messages }) => {
          const dialogueCalled = steps.some((s) =>
            s.toolCalls?.some((tc) => tc.toolName === "generateDialogueStep"),
          );
          if (dialogueCalled) {
            nudgeState.count = 0;
            nudgeState.timeReminded = false;
            return undefined;
          }

          const timeCalled = steps.some((s) =>
            s.toolCalls?.some((tc) => tc.toolName === "advanceTime"),
          );

          // Collect tool names preserving order
          const allToolsUsed: string[] = [];
          for (const s of steps) {
            const names = s.toolCalls?.map((tc) => tc.toolName) ?? [];
            for (const name of names) allToolsUsed.push(name);
          }

          // Group consecutive identical tool names
          const grouped: string[] = [];
          let i = 0;
          while (i < allToolsUsed.length) {
            const current = allToolsUsed[i];
            let runLen = 1;
            while (i + runLen < allToolsUsed.length && allToolsUsed[i + runLen] === current) {
              runLen++;
            }
            grouped.push(runLen > 1 ? `${current} (${runLen} times)` : current);
            i += runLen;
          }

          nudgeState.count++;
          const prefix = nudgeState.count === 1 ? "Reminder:" : "ERROR:";
          const toolList = grouped.length > 0 ? ` You called [${grouped.join(", ")}] but` : " You";
          let errorMsg = `${prefix}${toolList} have not yet called generateDialogueStep. The player cannot see any response. You MUST call generateDialogueStep now.`;

          // Soft one-time reminder for advanceTime on step 3+
          if (!timeCalled && !nudgeState.timeReminded && steps.length >= 3) {
            nudgeState.timeReminded = true;
            errorMsg +=
              "\n\nReminder: You can call advanceTime() if the player's action takes significant time. Skip if not needed.";
          }

          return { messages: [...messages, { role: "user" as const, content: errorMsg }] };
        }
      )({ count: 0, timeReminded: false }),
    });

    let toolRawArgs = "";
    let dialogueToolId: string | null = null;
    let hasEmittedStreaming = false;
    for await (const chunk of result.fullStream) {
      switch (chunk.type) {
        case "tool-input-start":
          if (chunk.toolName === "generateDialogueStep") {
            if (hasEmittedStreaming) {
              events.emitStreamingReset();
            }
            dialogueToolId = chunk.id;
            toolRawArgs = "";
            hasEmittedStreaming = false;
          }
          break;
        case "tool-input-delta":
          if (chunk.id === dialogueToolId) {
            toolRawArgs += chunk.delta;
            try {
              const parsed = parsePartial(toolRawArgs);
              if (parsed.messages && Array.isArray(parsed.messages) && parsed.messages.length > 0) {
                finalMessages = parsed.messages;
                hasEmittedStreaming = true;
                events.emitStreamingMessages(
                  finalMessages.map((m: any) => ({
                    speaker: m.speaker || "SYSTEM",
                    type: m.type || "SYSTEM",
                    text: m.text || "",
                    metadata: m.metadata,
                  })),
                );
              }
              if (parsed.options && Array.isArray(parsed.options)) {
                finalOptions = parsed.options.map((o: any, i: number) => ({
                  id: o.id || `opt_${i}`,
                  text: o.text || "",
                  selectionMessage: o.selectionMessage,
                  hintBefore: o.hintBefore,
                  hintAfter: o.hintAfter,
                  check: o.check
                    ? {
                        skill: o.check.skill,
                        difficulty: o.check.difficulty,
                        difficultyText: o.check.difficultyText || "",
                        diceCount: o.check.diceCount ?? 2,
                        isRed: o.check.isRed,
                        conditions: (o.check.conditions || []).map((c: any, ci: number) => ({
                          expression: c.expression,
                          label: c.label,
                          color: c.color,
                          stepId: c.stepId || `step_res_${ci}`,
                        })),
                      }
                    : undefined,
                }));
                if (finalOptions.length > 0) {
                  events.emitOptions(finalOptions);
                }
              }
            } catch {
              // Partial JSON not parseable yet — fine
            }
          }
          break;
        case "error":
          streamError =
            chunk.error instanceof Error
              ? chunk.error.message
              : String(chunk.error ?? "Unknown stream error");
          console.error(`[generateTurn] stream error: ${streamError}`);
          break;
        case "tool-call":
          if (chunk.toolName === "generateDialogueStep") {
            let args: Record<string, unknown> | null = null;
            if (typeof chunk.input === "string" && chunk.input.trim()) {
              const repaired = chunk.input.replace(/\]\s*\}\s*,\s*"/g, '], "');
              try {
                args = parsePartial(repaired) as Record<string, unknown>;
              } catch {
                try {
                  args = parsePartial(chunk.input) as Record<string, unknown>;
                } catch {
                  console.warn("[generateTurn] parsePartial recovery failed");
                }
              }
            } else if (chunk.input && typeof chunk.input === "object") {
              args = chunk.input as Record<string, unknown>;
            }
            if (args) {
              if (args.messages && Array.isArray(args.messages)) {
                finalMessages = args.messages as Record<string, unknown>[];
              }
              if (args.options && Array.isArray(args.options)) {
                finalOptions = (args.options as Record<string, unknown>[]).map((o, i) => ({
                  id: (o.id as string) || `opt_${i}`,
                  text: (o.text as string) || "",
                  selectionMessage: o.selectionMessage as string | undefined,
                  hintBefore: o.hintBefore as string | undefined,
                  hintAfter: o.hintAfter as string | undefined,
                  check: o.check
                    ? {
                        skill: (o.check as any).skill as SkillName,
                        difficulty: (o.check as any).difficulty as number,
                        difficultyText: ((o.check as any).difficultyText as string) || "",
                        diceCount: ((o.check as any).diceCount as number) ?? 2,
                        isRed: (o.check as any).isRed as boolean | undefined,
                        conditions: (((o.check as any).conditions as any[]) || []).map(
                          (c: any, ci: number) => ({
                            expression: c.expression as string,
                            label: c.label as string | undefined,
                            color: c.color as string | undefined,
                            stepId: (c.stepId as string) || `step_res_${ci}`,
                          }),
                        ),
                      }
                    : undefined,
                }));
              }
            }
          }
          break;
      }
    }
  } catch (error: unknown) {
    const err = error instanceof Error ? error : new Error(String(error));
    events.emitError(err.message);
    events.finish();
    return;
  }

  if (finalMessages.length === 0) {
    const msg = streamError
      ? `Generation failed: ${streamError}`
      : "Failed to generate valid dialogue";
    events.emitError(msg);
    events.finish();
    return;
  }

  const messages: Message[] = finalMessages.map((m: any, i) => ({
    id: `msg_${Date.now()}_${i}`,
    speaker: m.speaker || "SYSTEM",
    type: (m.type as Message["type"]) || "SYSTEM",
    text: m.text || "",
    metadata: m.metadata,
  }));

  events.emitParsed(
    messages.map((m) => ({ speaker: m.speaker, type: m.type, text: m.text, metadata: m.metadata })),
    finalOptions,
  );
  events.emitOptions(finalOptions);
  events.finish();

  // Persist current options so the player can resume from this point
  if (finalOptions.length > 0) {
    saveCurrentOptions(finalOptions).catch((err) =>
      console.error("[generateTurn] failed to persist options:", err),
    );
  }
}
