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
import { createDeepSeek } from "@ai-sdk/deepseek";
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { streamText, generateText, stepCountIs, type LanguageModel, type ModelMessage } from "ai";
import { parse as parsePartial } from "partial-json";
import type { Response } from "express";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { Character } from "@/types/entities";
import { getAllEntities, getAllEntitySummaries } from "@/server/models/world";
import { getAllPlots, addPlot, getPlotById } from "@/server/models/plot";
import type { Plot, PlotStatus } from "@/types/plot";
import { getFactsSnapshot } from "@/server/models/facts";
import {
  saveStep,
  deactivateSiblingBranches,
  updateOptionNextStepId,
  addOptionToStep,
} from "@/server/models/dialogue";
import { addMessage } from "@/server/models/history";
import { LlmDebugIntegration } from "@/server/llm/debug";
import { TurnEventEmitter } from "@/server/llm/events";
import {
  mapToDialogueOption,
  createListEntitiesTool,
  createGetEntityTool,
  createUpdateEntityTool,
  createUpdateEntitiesTool,
  createCreateEntityTool,
  createGetCharacterStateTool,
  createUpdateCharacterStateTool,
  createCreatePlotTool,
  createUpdatePlotTool,
  createGetPlotTool,
  createGenerateDialogueStepTool,
  createAdvanceTimeTool,
  createUpdateSceneTool,
  createGetSceneTool,
  createAddFactTool,
  createGetFactTool,
  createUpdateFactTool,
  createRemoveFactTool,
} from "@/server/llm/tools";
import { TOOL_NAMES } from "@/shared/constants";
import { getSceneState, getGameTime } from "@/server/models/scene";
import { buildSystemPrompt } from "@/server/llm/prompt";
import { getActiveSeedStory } from "@/server/seed-stories";

// ── Constants ──

const MAX_GM_STEPS = 10;

// ── Model management ──

let googleModelInstance: LanguageModel | null = null;
let deepseekModelInstance: LanguageModel | null = null;

function getGoogleModel(): LanguageModel | null {
  if (!googleModelInstance && process.env.GEMINI_API_KEY) {
    try {
      googleModelInstance = createGoogleGenerativeAI({
        apiKey: process.env.GEMINI_API_KEY,
      })("gemini-2.0-flash-lite-preview-02-05");
    } catch (e) {
      console.error("Failed to initialize Google model:", e);
    }
  }
  return googleModelInstance;
}

function getDeepSeekModel(): LanguageModel | null {
  if (!deepseekModelInstance && process.env.DEEPSEEK_API_KEY) {
    try {
      deepseekModelInstance = createDeepSeek({
        apiKey: process.env.DEEPSEEK_API_KEY,
      })("deepseek-v4-flash");
    } catch (e) {
      console.error("Failed to initialize DeepSeek model:", e);
    }
  }
  return deepseekModelInstance;
}

export function getModel(): { model: LanguageModel; name: string } {
  const google = getGoogleModel();
  if (google) return { model: google, name: "gemini-2.0-flash" };
  const deepseek = getDeepSeekModel();
  if (deepseek) return { model: deepseek, name: "deepseek-v4-flash" };
  throw new Error("Missing API Key: Please set GEMINI_API_KEY or DEEPSEEK_API_KEY in .env");
}

// ── System prompt (see ./prompt.ts) ──

export {
  DEFAULT_SYSTEM_PROMPT_TEMPLATE,
  getSystemPromptTemplate,
  setSystemPromptTemplate,
  buildSystemPrompt,
} from "@/server/llm/prompt";

// ── Shared tool set ──

function createAllTools(
  events: TurnEventEmitter,
  dialogueTool: ReturnType<typeof createGenerateDialogueStepTool>["tool"],
) {
  return {
    listEntities: createListEntitiesTool(),
    getEntity: createGetEntityTool(),
    updateEntity: createUpdateEntityTool(events),
    updateEntities: createUpdateEntitiesTool(events),
    createEntity: createCreateEntityTool(events),
    getCharacterState: createGetCharacterStateTool(),
    updateCharacterState: createUpdateCharacterStateTool(events),
    createPlot: createCreatePlotTool(events),
    updatePlot: createUpdatePlotTool(events),
    getPlot: createGetPlotTool(),
    getScene: createGetSceneTool(),
    updateScene: createUpdateSceneTool(events),
    advanceTime: createAdvanceTimeTool(events),
    addFact: createAddFactTool(events),
    getFact: createGetFactTool(),
    updateFact: createUpdateFactTool(events),
    removeFact: createRemoveFactTool(events),
    generateDialogueStep: dialogueTool,
  };
}

// ── Game Master ──

function persistStep(
  stepId: string,
  parentStepId: string | null,
  parentOptionId: string | null,
  messages: Message[],
  options: DialogueOption[],
  playerCharacter: Character | null,
  label: string,
  userInput: string | null,
) {
  // Custom input: parentStepId set but no parentOptionId → create a synthetic option
  // on the parent step so this branch is navigable in replay mode.
  let effectiveParentOptionId = parentOptionId;
  if (parentStepId && !effectiveParentOptionId) {
    const customOptionId = `custom_${nextId()}`;
    const optionText = (userInput ?? "Custom input").slice(0, 120);
    const customOption: DialogueOption = {
      id: customOptionId,
      text: optionText.length >= 120 ? optionText.slice(0, 117) + "…" : optionText,
      selectionMessage: userInput ?? "Custom input",
    };
    addOptionToStep(parentStepId, customOption);
    effectiveParentOptionId = customOptionId;
    console.log(`[${label}] synthetic custom option: ${parentStepId}.${customOptionId}`);
  }

  saveStep({
    id: stepId,
    parentStepId,
    parentOptionId: effectiveParentOptionId,
    messages,
    options,
    worldSnapshot: {
      entities: getAllEntities(),
      plots: getAllPlots(),
      playerCharacter,
      gameTime: getGameTime(),
      scene: getSceneState(),
      facts: getFactsSnapshot(),
    },
    isGenerated: true,
    isActive: true,
  });

  console.log(
    `[${label}] persisted step=${stepId} messages=${messages.length} options=${options.length}`,
  );

  if (parentStepId && effectiveParentOptionId) {
    updateOptionNextStepId(parentStepId, effectiveParentOptionId, stepId);
    console.log(
      `[${label}] linked parent option: ${parentStepId}.${effectiveParentOptionId} -> ${stepId}`,
    );
  }

  for (const msg of messages) {
    try {
      addMessage(msg);
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      if (!message.includes("UNIQUE constraint failed")) {
        console.error("Failed to save message to history:", message);
      }
    }
  }

  if (parentStepId) {
    deactivateSiblingBranches(parentStepId, stepId);
  }
}

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
              const repaired = chunk.input.replace(/\]\s*\}\s*,\s*"/g, '], "');
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
  const noopEvents = new TurnEventEmitter(null, stepId);
  const dialogueStepTool = createGenerateDialogueStepTool(noopEvents);

  const result = await generateText({
    model,
    system: systemPrompt,
    messages: [{ role: "user", content: promptText }],
    tools: createAllTools(noopEvents, dialogueStepTool.tool),
  });

  // Extract the generateDialogueStep tool input
  const dialogueCall = result.toolCalls?.find((tc) => tc.toolName === TOOL_NAMES.GENERATE_DIALOGUE);
  const rawInput = dialogueCall?.input;

  // Recover from malformed JSON with tolerant partial-json parser.
  // Also repair a known LLM bug: premature `}` after messages array
  let args: Record<string, unknown> | null = null;
  if (typeof rawInput === "string" && rawInput.trim()) {
    const repaired = rawInput.replace(/\]\s*\}\s*,\s*"/g, '], "');
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

// ── Plot tree pre-generation ──

interface PlotDef {
  index: number;
  title: string;
  description: string;
  status: string;
  involvedLocations: string[];
  involvedCharacters: string[];
  childPlots: Array<{ childPlotIndex: number | null; triggerCondition: string }>;
}

export async function pregeneratePlotTree(size: number): Promise<Plot[]> {
  const seedStory = getActiveSeedStory();
  const summaries = getAllEntitySummaries();

  const entityLines = summaries
    .map((e) => `  ${e.id} — "${e.displayName}" (${e.type}) — ${e.shortDescription}`)
    .join("\n");

  const prompt = [
    `You are designing a complete plot tree for a narrative RPG.`,
    ``,
    `SETTING: ${seedStory.settingDescription}`,
    `TONE: ${seedStory.toneDescription}`,
    ``,
    `Available entities (use these exact IDs):`,
    entityLines,
    ``,
    `Generate a plot tree with approximately ${size} nodes. Follow these rules:`,
    ``,
    `1. Plot index 0 is the ROOT — it has no parent. It represents the overarching story.`,
    `2. Each plot is a BROAD narrative arc (chapter/quest), not a single scene or dialogue beat.`,
    `3. childPlots define narrative branch directions — each triggerCondition describes a story-level choice ("Player sides with the rebels"), not a specific dialogue line.`,
    `4. Status must be "PENDING" for all nodes.`,
    `5. The tree should be SHALLOW and WIDE — prefer 2-3 levels with many branches over deep nesting.`,
    `6. Use exact entity IDs from the list above for involvedLocations and involvedCharacters.`,
    `7. Every non-root plot must be referenced by exactly one parent's childPlots via childPlotIndex.`,
    `8. Each non-leaf node should have 2-5 childPlots (some with childPlotIndex set to a valid index, some null for future expansion).`,
    `9. The tree must be connected — every node must be reachable from the root.`,
    ``,
    `Output ONLY valid JSON in this exact format (no markdown, no explanation):`,
    `{"plots":[{"index":0,"title":"string","description":"string","status":"PENDING","involvedLocations":["id"],"involvedCharacters":["id"],"childPlots":[{"childPlotIndex":1,"triggerCondition":"string"},{"childPlotIndex":null,"triggerCondition":"string"}]}]}`,
  ].join("\n");

  const { model } = getModel();
  const result = await generateText({ model, messages: [{ role: "user", content: prompt }] });

  // Strip markdown code fences before extracting JSON — the regex /\{[\s\S]*\}/
  // would otherwise match across fences and include non-JSON text.
  let text = result.text.trim();
  text = text.replace(/^```(?:json)?\s*\n?/gm, "").replace(/```\s*$/gm, "").trim();
  const jsonMatch = text.match(/\{[\s\S]*\}/);
  if (!jsonMatch) throw new Error("Failed to parse plot tree JSON from LLM response");

  let parsed: { plots: PlotDef[] };
  try {
    parsed = JSON.parse(jsonMatch[0]);
  } catch {
    throw new Error("Invalid JSON in LLM response for plot tree");
  }

  if (!Array.isArray(parsed.plots) || parsed.plots.length === 0) {
    throw new Error("No plots in generated tree");
  }

  const plotDefs = parsed.plots;

  // ── Validate LLM output before any side effects ──────────────────────────

  // Duplicate indices would silently corrupt the index→ID map
  const indexSet = new Set<number>();
  for (const def of plotDefs) {
    if (indexSet.has(def.index)) {
      throw new Error(`Duplicate plot index ${def.index} in generated tree`);
    }
    indexSet.add(def.index);
  }

  // Root (index 0) must exist
  if (!indexSet.has(0)) {
    throw new Error("Root plot (index 0) is required but missing from generated tree");
  }

  // Every childPlotIndex must reference an existing plot index
  for (const def of plotDefs) {
    for (const cp of def.childPlots) {
      if (cp.childPlotIndex !== null && !indexSet.has(cp.childPlotIndex)) {
        throw new Error(
          `Plot ${def.index} references non-existent childPlotIndex ${cp.childPlotIndex}`,
        );
      }
    }
  }

  // Every non-root plot must be reachable from root via childPlots
  const referenced = new Set<number>();
  for (const def of plotDefs) {
    for (const cp of def.childPlots) {
      if (cp.childPlotIndex !== null) referenced.add(cp.childPlotIndex);
    }
  }
  for (const def of plotDefs) {
    if (def.index !== 0 && !referenced.has(def.index)) {
      throw new Error(
        `Plot ${def.index} ("${def.title}") is not referenced by any parent's childPlots — orphans are not allowed`,
      );
    }
  }

  // ── Assign real IDs ──────────────────────────────────────────────────────

  const indexToId = new Map<number, string>();
  for (const def of plotDefs) {
    indexToId.set(def.index, `plot_${nextId()}`);
  }

  // Build child → parent mapping
  const childParentMap = new Map<number, { parentIndex: number; optionIndex: number }>();
  for (const def of plotDefs) {
    for (let i = 0; i < def.childPlots.length; i++) {
      const child = def.childPlots[i];
      if (child.childPlotIndex !== null) {
        childParentMap.set(child.childPlotIndex, { parentIndex: def.index, optionIndex: i });
      }
    }
  }

  // Topological sort: parent before children
  const visited = new Set<number>();
  const order: number[] = [];

  function visit(index: number) {
    if (visited.has(index)) return;
    const parentInfo = childParentMap.get(index);
    if (parentInfo) visit(parentInfo.parentIndex);
    visited.add(index);
    order.push(index);
  }

  for (const def of plotDefs) {
    visit(def.index);
  }

  // Insert in order
  const inserted: Plot[] = [];
  for (const index of order) {
    const def = plotDefs.find((d) => d.index === index)!;

    const id = indexToId.get(index)!;
    const parentInfo = childParentMap.get(index);
    const parentPlotId = parentInfo ? indexToId.get(parentInfo.parentIndex)! : null;
    const parentOptionId = parentInfo ? parentInfo.optionIndex : null;

    // Set all plotIds to null — addPlot's auto-link mechanism updates the
    // parent's childPlots when each child is inserted, so links are wired
    // incrementally without triggering validatePlotTree failures from
    // forward-references to not-yet-inserted children.
    const childPlots = def.childPlots.map((cp) => ({
      plotId: null,
      triggerCondition: cp.triggerCondition,
    }));

    const addResult = addPlot({
      id,
      title: def.title,
      description: def.description,
      status: (def.status as PlotStatus) ?? "PENDING",
      involvedLocations: def.involvedLocations ?? [],
      involvedCharacters: def.involvedCharacters ?? [],
      parentPlotId,
      parentOptionId,
      childPlots,
    });

    if ("error" in addResult) {
      throw new Error(`Failed to insert plot "${def.title}": ${addResult.error}`);
    }

    const plot = getPlotById(id);
    if (plot) inserted.push(plot);
  }

  return inserted;
}
