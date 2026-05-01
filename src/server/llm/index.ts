import { createDeepSeek } from "@ai-sdk/deepseek";
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { streamText, generateText, type LanguageModel } from "ai";
import { parse as parsePartial } from "partial-json";
import type { Response } from "express";
import type { Message, DialogueOption } from "@/types/dialogue";
import { getAllEntities } from "@/server/models/world";
import { getAllPlots } from "@/server/models/plot";
import { saveStep, deactivateSiblingBranches, updateOptionNextStepId } from "@/server/models/dialogue";
import { addMessage } from "@/server/models/history";
import { LlmDebugIntegration } from "@/server/llm/debug";
import { TurnEventEmitter } from "@/server/llm/events";
import {
  mapToDialogueOption,
  createUpdateWorldStateTool,
  createUpdatePlotStatusTool,
  createCreatePlotTool,
  createGenerateDialogueStepTool,
} from "@/server/llm/tools";

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
  throw new Error(
    "Missing API Key: Please set GEMINI_API_KEY or DEEPSEEK_API_KEY in .env",
  );
}

// ── System prompt ──

export function buildSystemPrompt(): string {
  const worldState = getAllEntities();
  const plots = getAllPlots();
  const activePlots = plots.filter(
    (p) => p.status === "PENDING" || p.status === "IN_PROGRESS",
  );

  return `
You are the Game Master for a narrative-driven RPG. You use different tool to interact with player. The people talking to you (i.e., user) is your assistant.
SETTING: A dark, gritty medieval world. High-contrast noir aesthetic.
TONE: Philosophical, cynical, and surreal. Mimic the writing style of Disco Elysium (if you have enough knowledge of the game).

---

## INTERNAL VOICES
- LOGIC: Cold, deductive.
- RHETORIC: Political, manipulative.
- VOLITION: Willpower and sanity.
- INLAND EMPIRE: Imagination, supra-natural hunches.
- HALF LIGHT: Pure lizard-brain fear.
- ELECTROCHEMISTRY: Hedonism, desire.

---

## OUTPUT FORMAT
**CRITICAL: ALWAYS call tool generateDialogueStep.**
When you call tools like updateWorldState, updatePlotStatus, or createPlot, do not forget to call generateDialogueStep, otherwise player will be stuck at the latest messages.

**CRITICAL: You MUST NEVER output raw text directly.**
Do NOT provide "thought" summaries, preambles, or conversational filler outside of the tool.
Every narrative turn MUST conclude with exactly one call to the \`generateDialogueStep\` tool.
The \`generateDialogueStep\` tool is the ONLY way to communicate to the player.
Any text outside this tool call will be DISCARDED and ignored by the system.
- speaker: Specifically the name of the speaker. Do NOT include the speaker name in the text field. For internal voices, use its name (e.g., "LOGIC"). For narrations, use "NARRATOR".
- type: MUST be one of "CHARACTER", "INNER_VOICE", "SYSTEM", "YOU", "NOTIFICATION".
  - Use "INNER_VOICE" for stats (LOGIC, VOLITION, etc.).
  - Use "SYSTEM" for the "NARRATOR" or general environment descriptions.
  - Use "CHARACTER" for NPCs.
- text: The actual dialogue or narration. Support Markdown formatting. Do NOT put speaker names at the beginning of the text field.
- Options MUST have "isAiTrigger":true if you want to let the AI continue the conversation.
- Options should be ACTION-ORIENTED ("Threaten the guard", "Sneak past", "Negotiate") — not wildly divergent.
- Call updateWorldState / updatePlotStatus / createPlot tools as needed to mutate the world BEFORE calling \`generateDialogueStep\`.
- If a skill check is appropriate, include the "check" object in the option.

BAD example (too divergent): [Option to fly to the moon], [Option to become a farmer]
GOOD example (action-oriented, same scene): [Intimidate the guard], [Bribe the guard], [Find another entrance]

BAD example (output dialogues in your thinking or raw response, player cannot see that): Tell player what has happened in the world, [Call updateWorldState]
GOOD example (action-oriented, same scene): [Call generateDialogueStep], [Call updateWorldState]

---

## WORLD STATE

\`\`\`json
${JSON.stringify(worldState, null, 2)}
\`\`\`

---

## PLOTS

\`\`\`json
${JSON.stringify(activePlots, null, 2)}
\`\`\`

Progress these plots when narratively appropriate. Create new plots if needed.

`.trim();
}

// ── Game Master ──

export async function generateTurn(
  userInput: string,
  history: Message[],
  res: Response,
  parentStepId: string | null,
  parentOptionId: string | null,
): Promise<void> {
  const systemPrompt = buildSystemPrompt();
  const stepId = `step_${Date.now()}`;
  const events = new TurnEventEmitter(res, stepId);

  console.log(`[generateTurn] stepId=${stepId} parentStepId=${parentStepId} parentOptionId=${parentOptionId} historyLen=${history.length} userInput="${String(userInput).slice(0, 80)}"`);

  res.writeHead(200, {
    "Content-Type": "text/event-stream",
    "Cache-Control": "no-cache",
    Connection: "keep-alive",
    "X-Accel-Buffering": "no",
  });

  events.startStep();

  const MAX_RETRIES = 3;
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

  const { model, name: modelName } = getModel();

  let finalMessages: Record<string, unknown>[] = [];
  let finalOptions: DialogueOption[] = [];
  let lastError: Error | null = null;
  let prevAttemptToolCalls: string[] = [];

  for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
    finalMessages = [];
    finalOptions = [];
    lastError = null;

    const attemptToolCalls: string[] = [];

    const retryNotice =
      attempt > 0
        ? prevAttemptToolCalls.length > 0
          ? `SYSTEM ERROR: Your previous response called [${prevAttemptToolCalls.join(", ")}] but never called generateDialogueStep. The player is stuck — generateDialogueStep is REQUIRED as the final call. Call it now.\n\n`
          : `SYSTEM ERROR: Your previous response did not call generateDialogueStep at all. The player is stuck — you MUST call generateDialogueStep. Call it now.\n\n`
        : "";

    const debugging = new LlmDebugIntegration(
      {
        model: modelName,
        system: systemPrompt,
        prompt: promptText,
        userInput,
        history,
        tools: ["updateWorldState", "updatePlotStatus", "createPlot", "generateDialogueStep"],
      },
      undefined,
      attempt > 0 ? `GM-retry${attempt}` : "GM",
    );

    try {
      const result = streamText({
        model,
        system: systemPrompt,
        messages: [{ role: "user", content: retryNotice + promptText }],
        tools: {
          updateWorldState: createUpdateWorldStateTool(events),
          updatePlotStatus: createUpdatePlotStatusTool(events),
          createPlot: createCreatePlotTool(events),
          generateDialogueStep: createGenerateDialogueStepTool(events),
        },
        onStepFinish: (event) => {
          for (const tc of event.toolCalls ?? []) {
            attemptToolCalls.push(tc.toolName);
          }
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
            text: event.text ?? undefined,
          });
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

      for await (const chunk of result.fullStream) {
        switch (chunk.type) {
          case "text-delta":
            break;

          case "tool-input-start":
            if (chunk.toolName === "generateDialogueStep") {
              dialogueToolId = chunk.id;
              toolRawArgs = "";
            }
            break;

          case "tool-input-delta":
            if (chunk.id === dialogueToolId) {
              toolRawArgs += chunk.delta;
              try {
                const parsed = parsePartial(toolRawArgs);
                if (parsed.messages && Array.isArray(parsed.messages)) {
                  finalMessages = parsed.messages;
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

          case "tool-call":
            if (chunk.toolName === "generateDialogueStep") {
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
                  finalOptions = (args.options as Record<string, unknown>[]).map(
                    (o, i) => mapToDialogueOption(o, i, stepId),
                  );
                }
              }
            }
            break;
        }
      }

      if (finalMessages.length > 0) {
        break; // success — exit retry loop
      }

      prevAttemptToolCalls = attemptToolCalls;
      console.warn(`[generateTurn] attempt ${attempt + 1}/${MAX_RETRIES}: no messages generated (tools called: [${attemptToolCalls.join(", ")}]), retrying...`);
    } catch (error: unknown) {
      lastError = error instanceof Error ? error : new Error(String(error));
      debugging.onError(lastError);
      prevAttemptToolCalls = attemptToolCalls;
      console.warn(`[generateTurn] attempt ${attempt + 1}/${MAX_RETRIES} errored: ${lastError.message}`);
    }
  }

  if (finalMessages.length === 0) {
    const errMsg = lastError
      ? `All ${MAX_RETRIES} generation attempts failed: ${lastError.message}`
      : `Failed to generate valid dialogue after ${MAX_RETRIES} attempts`;
    events.emitError(errMsg);
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

  // Persist
  saveStep({
    id: stepId,
    parentStepId,
    parentOptionId,
    messages,
    options: finalOptions,
    worldSnapshot: getAllEntities() as unknown as Record<string, unknown>,
    isGenerated: true,
    isActive: true,
  });

  console.log(`[generateTurn] persisted step=${stepId} messages=${messages.length} options=${finalOptions.length}`);

  if (parentStepId && parentOptionId) {
    updateOptionNextStepId(parentStepId, parentOptionId, stepId);
    console.log(`[generateTurn] linked parent option: ${parentStepId}.${parentOptionId} -> ${stepId}`);
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

  events.finish();
}

// ── Batch generation (non-streaming, for bulk regenerate) ──

export async function generateTurnBatch(
  userInput: string,
  history: Message[],
  parentStepId: string | null,
  parentOptionId: string | null,
): Promise<{ stepId: string; messages: Message[]; options: DialogueOption[] }> {
  const systemPrompt = buildSystemPrompt();
  const stepId = `step_${Date.now()}`;

  console.log(`[generateTurnBatch] stepId=${stepId} parentStepId=${parentStepId} parentOptionId=${parentOptionId} historyLen=${history.length}`);

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

  const result = await generateText({
    model,
    system: systemPrompt,
    messages: [{ role: "user", content: promptText }],
    tools: {
      updateWorldState: createUpdateWorldStateTool(noopEvents),
      updatePlotStatus: createUpdatePlotStatusTool(noopEvents),
      createPlot: createCreatePlotTool(noopEvents),
      generateDialogueStep: createGenerateDialogueStepTool(noopEvents),
    },
  });

  // Extract the generateDialogueStep tool input
  const dialogueCall = result.toolCalls?.find(
    (tc) => tc.toolName === "generateDialogueStep",
  );
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
  const finalOptions: DialogueOption[] = (
    (args?.options as Record<string, unknown>[]) ?? []
  ).map((o, i) => mapToDialogueOption(o, i, stepId));

  const messages: Message[] = finalMessages.map((m: any, i) => ({
    id: `msg_${stepId}_${i}`,
    speaker: m.speaker || "SYSTEM",
    type: (m.type as Message["type"]) || "SYSTEM",
    text: m.text || "",
    metadata: m.metadata,
  }));

  // Persist
  saveStep({
    id: stepId,
    parentStepId,
    parentOptionId,
    messages,
    options: finalOptions,
    worldSnapshot: getAllEntities() as unknown as Record<string, unknown>,
    isGenerated: true,
    isActive: true,
  });

  console.log(`[generateTurnBatch] persisted step=${stepId} messages=${messages.length} options=${finalOptions.length}`);

  if (parentStepId && parentOptionId) {
    updateOptionNextStepId(parentStepId, parentOptionId, stepId);
    console.log(`[generateTurnBatch] linked parent option: ${parentStepId}.${parentOptionId} -> ${stepId}`);
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

  return { stepId, messages, options: finalOptions };
}
