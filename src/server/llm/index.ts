import { createDeepSeek } from "@ai-sdk/deepseek";
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { streamText, type LanguageModel } from "ai";
import { parse as parsePartial } from "partial-json";
import type { Response } from "express";
import type { Message, DialogueOption } from "@/types/dialogue";
import { getAllEntities } from "@/server/models/world";
import { getAllPlots } from "@/server/models/plot";
import { saveStep, deactivateSiblingBranches } from "@/server/models/dialogue";
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
You are the Game Master for a narrative-driven RPG.
SETTING: A dark, gritty medieval world. High-contrast noir aesthetic.
TONE: Philosophical, cynical, and surreal. Mimic the writing style of Disco Elysium.

---

## INTERNAL VOICES
- LOGIC: Cold, deductive.
- RHETORIC: Political, manipulative.
- VOLITION: Willpower and sanity.
- INLAND EMPIRE: Imagination, supra-natural hunches.
- HALF LIGHT: Pure lizard-brain fear.
- ELECTROCHEMISTRY: Hedonism, desire.

---

## WORLD STATE
${JSON.stringify(worldState, null, 2)}

---

## PLOTS
${JSON.stringify(activePlots, null, 2)}

Progress these plots when narratively appropriate. Create new plots if needed.

---

## OUTPUT FORMAT
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

  res.writeHead(200, {
    "Content-Type": "text/event-stream",
    "Cache-Control": "no-cache",
    Connection: "keep-alive",
    "X-Accel-Buffering": "no",
  });

  events.startStep();

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
    "GM",
  );

  try {
    const result = streamText({
      model,
      system: systemPrompt,
      messages: [{ role: "user", content: promptText }],
      tools: {
        updateWorldState: createUpdateWorldStateTool(events),
        updatePlotStatus: createUpdatePlotStatusTool(events),
        createPlot: createCreatePlotTool(events),
        generateDialogueStep: createGenerateDialogueStepTool(events),
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

    // Stream processing: accumulate tool-input-delta for generateDialogueStep
    let toolRawArgs = "";
    let dialogueToolId: string | null = null;
    let finalMessages: Record<string, unknown>[] = [];
    let finalOptions: DialogueOption[] = [
      { id: "opt_continue", text: "Continue", isAiTrigger: true },
    ];

    for await (const chunk of result.fullStream) {
      switch (chunk.type) {
        case "text-delta":
          // Silently discard — LLM text output is forbidden per system prompt
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
          // Full tool call available — use definitive args if we missed deltas
          if (chunk.toolName === "generateDialogueStep") {
            const args = chunk.input as {
              messages?: Record<string, unknown>[];
              options?: Record<string, unknown>[];
            };
            if (args.messages) {
              finalMessages = args.messages;
            }
            if (args.options) {
              finalOptions = args.options.map((o, i) =>
                mapToDialogueOption(o, i, stepId),
              );
            }
          }
          break;
      }
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
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    debugging.onError(error instanceof Error ? error : new Error(message));
    events.emitError(message);
    events.finish();
  }
}
