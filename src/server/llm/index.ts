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

import { nextId } from "@/server/models/ids";
import { streamText, generateText, stepCountIs } from "ai";
import { parse as parsePartial } from "partial-json";
import type { Response } from "express";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { Character } from "@/types/entities";
import { LlmDebugIntegration } from "@/server/llm/debug";
import { TurnEventEmitter, NoopEventEmitter } from "@/server/llm/events";
import { mapToDialogueOption, createGenerateDialogueStepTool } from "@/server/llm/tools";
import { TOOL_NAMES } from "@/shared/constants";
import { buildSystemPrompt } from "@/server/llm/prompt";
import { getModel } from "@/server/llm/model";
import { persistStep } from "@/server/llm/persistStep";
import { createAllTools } from "@/server/llm/toolsFactory";

// ── Constants ──

const MAX_GM_STEPS = 10;

// ── System prompt (see ./prompt.ts) ──

export {
  DEFAULT_SYSTEM_PROMPT_TEMPLATE,
  getSystemPromptTemplate,
  setSystemPromptTemplate,
  buildSystemPrompt,
} from "@/server/llm/prompt";

// ── Plot tree pre-generation ──

export { generatePlotDefs, type PlotDef } from "@/server/llm/pregeneratePlotTree";

// ── Game Master ──

export async function generateTurn(
  userInput: string,
  history: Message[],
  res: Response,
  parentStepId: string | null,
  parentOptionId: string | null,
  playerCharacter: Character | null = null,
): Promise<void> {
  const systemPrompt = buildSystemPrompt();
  const stepId = `step_${nextId()}`;
  const events = new TurnEventEmitter(res, stepId);

  console.log(
    `[generateTurn] stepId=${stepId} parentStepId=${parentStepId} parentOptionId=${parentOptionId} historyLen=${history.length} userInput="${String(userInput).slice(0, 80)}"`,
  );

  res.writeHead(200, {
    "Content-Type": "text/event-stream",
    "Cache-Control": "no-cache",
    Connection: "keep-alive",
    "X-Accel-Buffering": "no",
  });

  events.startStep();

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

  const { model, name: modelName } = getModel();

  let finalMessages: Record<string, unknown>[] = [];
  let finalOptions: DialogueOption[] = [];

  const dialogueStepTool = createGenerateDialogueStepTool(events);

  const debugging = new LlmDebugIntegration(
    {
      model: modelName,
      system: systemPrompt,
      prompt: promptText,
      userInput,
      history,
      tools: Object.values(TOOL_NAMES),
    },
    undefined,
    "GM",
  );

  let streamError: string | null = null;

  // Accumulators for per-step text/reasoning from the stream
  let currentText = "";
  let currentReasoning = "";
  const stepUserPrompts = new Map<number, string>();

  try {
    const result = streamText({
      model,
      system: systemPrompt,
      messages: [{ role: "user", content: promptText }],
      tools: createAllTools(events, dialogueStepTool.tool),
      stopWhen: [
        (state) => {
          const called = state.steps.some((s) =>
            s.toolCalls?.some((tc) => tc.toolName === TOOL_NAMES.GENERATE_DIALOGUE),
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
          s.toolCalls?.some((tc) => tc.toolName === TOOL_NAMES.GENERATE_DIALOGUE),
        );
        if (dialogueCalled) {
          stepUserPrompts.set(stepNumber, JSON.stringify(messages));
          return undefined;
        }
        const allToolsUsed = steps.flatMap((s) => s.toolCalls?.map((tc) => tc.toolName) ?? []);
        const errorMsg =
          allToolsUsed.length > 0
            ? `ERROR: You called [${allToolsUsed.join(", ")}] but never called ${TOOL_NAMES.GENERATE_DIALOGUE}. The player cannot see any response. You MUST call ${TOOL_NAMES.GENERATE_DIALOGUE} now.`
            : `ERROR: You did not call ${TOOL_NAMES.GENERATE_DIALOGUE}. The player cannot see any response. You MUST call ${TOOL_NAMES.GENERATE_DIALOGUE} now.`;
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
          if (chunk.toolName === TOOL_NAMES.GENERATE_DIALOGUE) {
            if (hasEmittedStreaming) {
              // A retry is starting — notify the client so it can show a visual reset
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
                finalOptions = parsed.options.map((o: any, i: number) =>
                  mapToDialogueOption(o, i, stepId),
                );
                if (finalOptions.length > 0) {
                  events.emitOptions(finalOptions);
                }
              }
            } catch {
              // Partial JSON may not be parseable yet — that's fine
            }
          }
          break;

        case "error":
          // A tool execution threw unexpectedly — capture the error so the
          // final-messages check surfaces the real reason instead of a generic message.
          streamError =
            chunk.error instanceof Error
              ? chunk.error.message
              : String(chunk.error ?? "Unknown stream error");
          console.error(`[generateTurn] stream error chunk: ${streamError}`);
          break;

        case "tool-call":
          if (chunk.toolName === TOOL_NAMES.GENERATE_DIALOGUE) {
            let args: Record<string, unknown> | null = null;

            // SDK may leave input as a raw string when JSON parsing fails —
            // try the more tolerant partial-json parser to recover.
            // Also repair a known LLM bug: premature `}` after messages array
            // e.g. {"messages": [...]}, "metadata": ... → {"messages": [...], "metadata": ...
            if (typeof chunk.input === "string" && chunk.input.trim()) {
              const repaired = chunk.input.replace(/]\s*}\s*,\s*"/g, '], "');
              try {
                args = parsePartial(repaired) as Record<string, unknown>;
              } catch {
                try {
                  args = parsePartial(chunk.input) as Record<string, unknown>;
                } catch {
                  console.warn(`[generateTurn] parsePartial recovery failed for tool-call input`);
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
                finalOptions = (args.options as Record<string, unknown>[]).map((o, i) =>
                  mapToDialogueOption(o, i, stepId),
                );
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

  // Emit final state
  const messages: Message[] = finalMessages.map((m: any, i) => ({
    id: `msg_${stepId}_${i}`,
    speaker: m.speaker || "SYSTEM",
    type: (m.type as Message["type"]) || "SYSTEM",
    text: m.text || "",
    metadata: m.metadata,
  }));

  events.emitParsed(
    messages.map((m) => ({
      speaker: m.speaker,
      type: m.type,
      text: m.text,
      metadata: m.metadata,
    })),
    finalOptions,
  );
  events.emitOptions(finalOptions);

  persistStep(
    stepId,
    parentStepId,
    parentOptionId,
    messages,
    finalOptions,
    playerCharacter,
    "generateTurn",
    userInput,
  );
  events.finish();
}

// ── Batch generation (non-streaming, for bulk regenerate) ──

export async function generateTurnBatch(
  userInput: string,
  history: Message[],
  parentStepId: string | null,
  parentOptionId: string | null,
  playerCharacter: Character | null = null,
): Promise<{ stepId: string; messages: Message[]; options: DialogueOption[] }> {
  const systemPrompt = buildSystemPrompt();
  const stepId = `step_${nextId()}`;

  console.log(
    `[generateTurnBatch] stepId=${stepId} parentStepId=${parentStepId} parentOptionId=${parentOptionId} historyLen=${history.length}`,
  );

  const historyWindow = 10;
  const promptText = [
    `## Dialogue History (Last ${historyWindow})`,
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
  const noopEvents = new NoopEventEmitter(stepId);

  const MAX_BATCH_ATTEMPTS = 2;
  let dialogueCall: { toolCallId: string; toolName: string; input: unknown } | undefined;

  for (let attempt = 0; attempt < MAX_BATCH_ATTEMPTS; attempt++) {
    const messages =
      attempt === 0
        ? [{ role: "user" as const, content: promptText }]
        : [
            {
              role: "user" as const,
              content:
                promptText +
                "\n\nERROR: You must call generateDialogueStep. The player cannot see any response without it. Call generateDialogueStep now.",
            },
          ];

    // Re-create dialogue tool to reset wasValid state
    const dialogueToolAttempt = createGenerateDialogueStepTool(noopEvents);

    const result = await generateText({
      model,
      system: systemPrompt,
      messages,
      tools: createAllTools(noopEvents, dialogueToolAttempt.tool),
    });

    dialogueCall = result.toolCalls?.find((tc) => tc.toolName === TOOL_NAMES.GENERATE_DIALOGUE);
    if (dialogueCall) break;
  }

  // Extract the generateDialogueStep tool input
  const rawInput = dialogueCall?.input;

  // Recover from malformed JSON with tolerant partial-json parser.
  // Also repair a known LLM bug: premature `}` after messages array
  let args: Record<string, unknown> | null = null;
  if (typeof rawInput === "string" && rawInput.trim()) {
    const repaired = rawInput.replace(/]\s*}\s*,\s*"/g, '], "');
    try {
      args = parsePartial(repaired) as Record<string, unknown>;
    } catch {
      try {
        args = parsePartial(rawInput) as Record<string, unknown>;
      } catch {
        console.warn("[generateTurnBatch] parsePartial recovery failed for tool input");
      }
    }
  } else if (rawInput && typeof rawInput === "object") {
    args = rawInput as Record<string, unknown>;
  }

  const finalMessages: Record<string, unknown>[] =
    (args?.messages as Record<string, unknown>[]) ?? [];
  const finalOptions: DialogueOption[] = ((args?.options as Record<string, unknown>[]) ?? []).map(
    (o, i) => mapToDialogueOption(o, i, stepId),
  );

  if (finalMessages.length === 0) {
    throw new Error("generateTurnBatch: failed to generate valid dialogue after retries");
  }

  const messages: Message[] = finalMessages.map((m: any, i) => ({
    id: `msg_${stepId}_${i}`,
    speaker: m.speaker || "SYSTEM",
    type: (m.type as Message["type"]) || "SYSTEM",
    text: m.text || "",
    metadata: m.metadata,
  }));

  persistStep(
    stepId,
    parentStepId,
    parentOptionId,
    messages,
    finalOptions,
    playerCharacter,
    "generateTurnBatch",
    userInput,
  );
  return { stepId, messages, options: finalOptions };
}
