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

import { streamText, stepCountIs, NoSuchToolError, type ModelMessage } from "ai";
import { parse as parsePartial } from "partial-json";
import { jsonrepair } from "jsonrepair";
import type { Response } from "express";
import type { Message, DialogueOption } from "@/types/dialogue";
import { TurnEventEmitter } from "@/server/llm/events";
import { buildSystemPrompt, MAX_GM_STEPS } from "@/server/llm/prompt";
import { buildSceneContext } from "@/server/llm/sceneContext";
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
import { loadGMMessages, saveGMMessages, getNextTurnNumber } from "@/server/llm/gmMessages";
import { createGenerateDialogueStepTool } from "@/server/llm/tools/generateDialogueStep";
import { createAdvanceTimeTool } from "@/server/llm/tools/advanceTime";
import { performSkillCheck } from "@/server/llm/rollSkillCheck";
import { type SkillName, TOOL_NAMES } from "@/shared/constants";

export async function generateTurn(
  userInput: string,
  history: Message[],
  res: Response,
  check?: DialogueOption["check"],
): Promise<void> {
  const systemPrompt = await buildSystemPrompt();
  const events = new TurnEventEmitter(res);

  // Pre-fetch scene context so the GM doesn't need to query for it
  let sceneContext = "";
  try {
    sceneContext = await buildSceneContext();
  } catch (err) {
    console.error("[generateTurn] Failed to build scene context:", err);
  }

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

  // Load previous GM conversation messages for multi-turn continuity
  let previousMessages: ModelMessage[] = [];
  let turnNumber = 1;
  try {
    previousMessages = await loadGMMessages();
    turnNumber = await getNextTurnNumber();
  } catch (err) {
    console.error("[generateTurn] Failed to load GM messages, starting fresh:", err);
  }

  const includeHistory = false;
  const historyWindow = 10;
  let historyParts: string[] = [];
  if (includeHistory) {
    historyParts.push(
      `## DIALOGUE HISTORY (Last ${historyWindow})`,
      history
        .slice(-historyWindow)
        .map((m) => `${m.speaker} (${m.type}): ${m.text}`)
        .join("\n"),
      "",
      "---",
      "",
    );
  }

  const actionParts: string[] = [
    "## PLAYER ACTION",
    `The player just said/did: "${userInput}"`,
    "",
    "---",
    "",
  ];

  let skillCheckParts: string[] = [];
  if (check) {
    // Auto-perform the skill check server-side
    let rollResult: Awaited<ReturnType<typeof performSkillCheck>> | null = null;
    try {
      rollResult = await performSkillCheck(check);
    } catch (err) {
      console.error("[generateTurn] Skill check failed:", err);
    }

    if (rollResult) {
      // Emit SSE event for console rendering
      events.emitRollResult(rollResult);

      // Persist ROLL message
      const rollText = [
        `Rolled ${check.diceCount}d6 + ${check.skill}(${rollResult.statBonus})`,
        `Dice: [${rollResult.dice.join(", ")}]`,
        `Total: ${rollResult.total} vs Difficulty: ${check.difficulty}`,
        `Result: ${rollResult.success ? "SUCCESS" : "FAILURE"}`,
      ].join(" | ");

      await MemoryClient.getCachedInstance().shortTerm.addMessage("system", rollText, {
        speaker: check.skill,
        type: "ROLL",
        rollResult: {
          skill: rollResult.skill as SkillName,
          difficulty: rollResult.difficulty,
          dice: rollResult.dice,
          total: rollResult.total,
          success: rollResult.success,
        },
      });

      // Inject the result into the prompt — GM narrates, no tool call needed
      skillCheckParts.push(
        "## SKILL CHECK RESULT",
        rollResult.narrativeSummary,
        "",
        `The player ${rollResult.success ? "succeeded" : "failed"} this skill check.`,
        `Narrate the ${rollResult.success ? "success" : "failure"} naturally via ${TOOL_NAMES.GENERATE_DIALOGUE}.${rollResult.success ? " The player's skill shines through." : " Make the failure interesting but keep the story moving."}`,
        "",
        "---",
        "",
      );
    }
  }

  let contextParts: string[] = [];
  if (sceneContext) {
    contextParts.push(sceneContext, "", "---", "");
  }

  const promptText = [
    ...historyParts,
    ...actionParts,
    ...skillCheckParts,
    ...contextParts,
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
    await client.shortTerm.addMessage(role, msg.text, {
      speaker: msg.speaker,
      type: msg.type,
      ...msg.metadata,
    });
  };

  const dialogueStepTool = createGenerateDialogueStepTool(persistMessage);
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

  const result = streamText({
    model,
    system: systemPrompt,
    messages: [...previousMessages, { role: "user" as const, content: promptText }],
    tools: allTools,
    stopWhen: [
      (state) => {
        const called = state.steps.some((s) =>
          s.toolCalls?.some((tc) => tc.toolName === TOOL_NAMES.GENERATE_DIALOGUE),
        );
        const valid = dialogueStepTool.wasValid();
        console.log(
          `[stopWhen] steps=${state.steps.length} called=${called} valid=${valid} stepToolNames=${JSON.stringify(state.steps.map((s) => s.toolCalls?.map((tc) => tc.toolName)))}`,
        );
        return called && valid;
      },
      stepCountIs(MAX_GM_STEPS),
    ],
    prepareStep: (
      (nudgeState: { count: number; timeReminded: boolean }) =>
      ({ steps, messages }) => {
        const dialogueCalled = steps.some((s) =>
          s.toolCalls?.some((tc) => tc.toolName === TOOL_NAMES.GENERATE_DIALOGUE),
        );
        console.log(
          `[prepareStep] stepNumber=${steps.length} nudgeStateCount=${nudgeState.count} dialogueCalled=${dialogueCalled} stepToolNames=${JSON.stringify(steps.map((s) => s.toolCalls?.map((tc) => tc.toolName)))}`,
        );
        if (dialogueCalled) {
          nudgeState.count = 0;
          nudgeState.timeReminded = false;
          return undefined;
        }

        const timeCalled = steps.some((s) =>
          s.toolCalls?.some((tc) => tc.toolName === TOOL_NAMES.ADVANCE_TIME),
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
        let errorMsg = `${prefix}${toolList} have not yet called ${TOOL_NAMES.GENERATE_DIALOGUE}. The player cannot see any response. You MUST call ${TOOL_NAMES.GENERATE_DIALOGUE} now.`;

        // Soft one-time reminder for advanceTime on step 3+
        if (!timeCalled && !nudgeState.timeReminded && steps.length >= 3) {
          nudgeState.timeReminded = true;
          errorMsg += `Reminder: You can call ${TOOL_NAMES.ADVANCE_TIME}() if the player's action takes significant time. Skip if not needed.`;
        }

        return { messages: [...messages, { role: "user" as const, content: errorMsg }] };
      }
    )({ count: 0, timeReminded: false }),
    experimental_repairToolCall: async ({ toolCall, error }) => {
      if (NoSuchToolError.isInstance(error)) {
        return null;
      }
      try {
        const inputStr =
          typeof toolCall.input === "string" ? toolCall.input : JSON.stringify(toolCall.input);
        const repaired = jsonrepair(inputStr);
        console.log(`[repairToolCall] repaired ${toolCall.toolName} JSON`);
        return { ...toolCall, input: repaired };
      } catch (e) {
        console.warn(`[repairToolCall] jsonrepair failed for ${toolCall.toolName}:`, e);
        return null;
      }
    },
  });

  let toolRawArgs = "";
  let dialogueToolId: string | null = null;
  let hasEmittedStreaming = false;
  try {
    for await (const chunk of result.fullStream) {
      switch (chunk.type) {
        case "tool-input-start":
          if (chunk.toolName === TOOL_NAMES.GENERATE_DIALOGUE) {
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
          if (chunk.toolName === TOOL_NAMES.GENERATE_DIALOGUE) {
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

  // Persist this turn's messages for multi-turn continuity
  try {
    const response = await result.response;
    await saveGMMessages(response.messages as ModelMessage[], turnNumber);
  } catch (err) {
    console.error("[generateTurn] Failed to save GM messages:", err);
  }

  // If no dialogue tool passed validation, discard any invalid partial content
  // captured during streaming (e.g. when MAX_GM_STEPS fallthrough occurs)
  const dialogueWasValid = dialogueStepTool.wasValid();
  if (!dialogueWasValid) {
    finalMessages = [];
    finalOptions = [];
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
