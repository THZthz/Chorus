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
import { LlmDebugIntegration } from "@/server/llm/debug";
import { TurnEventEmitter } from "@/server/llm/events";
import { buildSystemPrompt, MAX_GM_STEPS } from "@/server/llm/prompt";
import { getModel } from "@/server/llm/model";
import { getMcpTools } from "@/server/mcp/client";
import { createGenerateDialogueStepTool } from "@/server/llm/generateDialogueStep";
import { createAdvanceTimeTool } from "@/server/llm/advanceTime";

export {
  DEFAULT_SYSTEM_PROMPT_TEMPLATE,
  getSystemPromptTemplate,
  setSystemPromptTemplate,
  buildSystemPrompt,
} from "@/server/llm/prompt";

export async function generateTurn(
  userInput: string,
  history: Message[],
  res: Response,
): Promise<void> {
  const systemPrompt = buildSystemPrompt();
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

  const historyWindow = 10;
  const promptText = [
    `## DIALOGUE HISTORY (Last ${historyWindow})`,
    history.slice(-historyWindow).map((m) => `${m.speaker} (${m.type}): ${m.text}`).join("\n"),
    "",
    "---",
    "",
    "## PLAYER ACTION",
    `The player just said/did: "${userInput}"`,
    "",
    "Generate the narrative response following the output format exactly.",
  ].join("\n");

  const { model, name: modelName } = getModel();

  let finalMessages: Record<string, unknown>[] = [];
  let finalOptions: DialogueOption[] = [];

  // Discover MCP tools from agent-memory
  const mcpTools = await getMcpTools();
  const dialogueStepTool = createGenerateDialogueStepTool(events);
  const advanceTimeTool = createAdvanceTimeTool(events);

  const allTools = {
    ...mcpTools,
    generateDialogueStep: dialogueStepTool.tool,
    advanceTime: advanceTimeTool,
  };

  const debugging = new LlmDebugIntegration(
    {
      model: modelName,
      system: systemPrompt,
      prompt: promptText,
      userInput,
      history,
      tools: [...Object.keys(mcpTools), "generateDialogueStep", "advanceTime"],
    },
    undefined,
    "GM",
  );

  let streamError: string | null = null;
  let currentText = "";
  let currentReasoning = "";
  const stepUserPrompts = new Map<number, string>();

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
      prepareStep: ({ stepNumber, steps, messages }) => {
        if (stepNumber === 0) {
          stepUserPrompts.set(stepNumber, JSON.stringify(messages));
          return undefined;
        }
        const dialogueCalled = steps.some((s) =>
          s.toolCalls?.some((tc) => tc.toolName === "generateDialogueStep"),
        );
        if (dialogueCalled) {
          stepUserPrompts.set(stepNumber, JSON.stringify(messages));
          return undefined;
        }
        const allToolsUsed = steps.flatMap((s) => s.toolCalls?.map((tc) => tc.toolName) ?? []);
        const errorMsg = allToolsUsed.length > 0
          ? `ERROR: You called [${allToolsUsed.join(", ")}] but never called generateDialogueStep. The player cannot see any response. You MUST call generateDialogueStep now.`
          : `ERROR: You did not call generateDialogueStep. You MUST call generateDialogueStep now.`;
        const newMessages = [...messages, { role: "user" as const, content: errorMsg }];
        stepUserPrompts.set(stepNumber, JSON.stringify(newMessages));
        return { messages: newMessages };
      },
      onStepFinish: (event) => {
        debugging.onStepFinish({
          stepNumber: event.stepNumber ?? 0,
          finishReason: event.finishReason ?? "unknown",
          usage: event.usage,
          toolCalls: event.toolCalls?.map((tc) => ({
            toolCallId: tc.toolCallId,
            toolName: tc.toolName,
            input: tc.input,
          })),
          toolResults: event.toolResults?.map((tr) => ({
            toolCallId: tr.toolCallId,
            toolName: tr.toolName,
            output: tr.output,
          })),
          text: event.text || currentText || undefined,
          reasoning: currentReasoning || undefined,
          userPrompt: stepUserPrompts.get(event.stepNumber ?? 0),
        });
        stepUserPrompts.delete(event.stepNumber ?? 0);
      },
      onFinish: (event) => {
        debugging.onFinish({
          finishReason: event.finishReason ?? "unknown",
          usage: event.usage,
          totalUsage: event.totalUsage,
          steps: event.steps,
          text: event.text ?? undefined,
        });
      },
    });

    let toolRawArgs = "";
    let dialogueToolId: string | null = null;
    let hasEmittedStreaming = false;
    for await (const chunk of result.fullStream) {
      switch (chunk.type) {
        case "text-start":
          currentText = "";
          break;
        case "reasoning-start":
          currentReasoning = "";
          break;
        case "text-delta":
          currentText += chunk.text;
          break;
        case "reasoning-delta":
          currentReasoning += chunk.text;
          break;
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
                  check: o.check ? {
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
                  } : undefined,
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
          streamError = chunk.error instanceof Error
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
                  check: o.check ? {
                    skill: (o.check as any).skill as string,
                    difficulty: (o.check as any).difficulty as number,
                    difficultyText: ((o.check as any).difficultyText as string) || "",
                    diceCount: ((o.check as any).diceCount as number) ?? 2,
                    isRed: (o.check as any).isRed as boolean | undefined,
                    conditions: (((o.check as any).conditions as any[]) || []).map((c: any, ci: number) => ({
                      expression: c.expression as string,
                      label: c.label as string | undefined,
                      color: c.color as string | undefined,
                      stepId: (c.stepId as string) || `step_res_${ci}`,
                    })),
                  } : undefined,
                }));
              }
            }
          }
          break;
      }
    }
  } catch (error: unknown) {
    const err = error instanceof Error ? error : new Error(String(error));
    debugging.onError(err);
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
}
