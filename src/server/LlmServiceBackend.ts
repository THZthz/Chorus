import { createDeepSeek } from "@ai-sdk/deepseek";
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { streamText, generateText, tool, type LanguageModel } from "ai";
import { z } from "zod";
import { parse as parsePartial } from "partial-json";
import type { Response } from "express";
import { Message, DialogueOption } from "@/types/dialogue";
import { getAllEntities } from "@/server/models/world";
import { getAllPlots } from "@/server/models/plot";
import { saveStep, deactivateSiblingBranches } from "@/server/models/dialogue";
import { addMessage, getHistory } from "@/server/models/history";
import { LlmDebugIntegration } from "@/server/LlmDebugIntegration";
import { TurnEventEmitter, parseResponseText } from "@/server/sseEvents";
import { createUpdateWorldStateTool } from "@/services/tools/updateWorldState";
import { createUpdatePlotStatusTool } from "@/services/tools/updatePlotStatus";
import { createCreatePlotTool } from "@/services/tools/createPlot";

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

function getModelInfo(): { model: LanguageModel; name: string } {
  const google = getGoogleModel();
  if (google) return { model: google, name: "gemini-2.0-flash" };
  const deepseek = getDeepSeekModel();
  if (deepseek) return { model: deepseek, name: "deepseek-v4-flash" };
  throw new Error(
    "Missing API Key: Please set GEMINI_API_KEY or DEEPSEEK_API_KEY in the application settings or .env file."
  );
}

function buildSystemPrompt(): string {
  const worldState = getAllEntities();
  const plots = getAllPlots();
  const activePlots = plots.filter((p) => p.status === "PENDING" || p.status === "IN_PROGRESS");

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
Generate your narrative response by utilizing the dialogue_response tool. You MUST use this tool to output the sequence of dialogue messages and any player choices.
- Speaker name can be: an internal voice (LOGIC, RHETORIC, VOLITION, INLAND EMPIRE, HALF LIGHT, ELECTROCHEMISTRY, SUGGESTION, CONCEPTUALIZATION, EMPATHY, VISUAL CALCULUS, PERCEPTION, ENDURANCE, PHYSICAL INSTRUMENT), a character name (Madam Vespera), or NARRATOR.
- Speaker types: YOU, INNER_VOICE, CHARACTER, SYSTEM, ROLL, NOTIFICATION.
- Text content should support Markdown formatting, and you can reference entities with [Name](#entity_id).
- Options MUST have "isAiTrigger":true if you want to let the AI continue the conversation.
- Options should be ACTION-ORIENTED ("Threaten the guard", "Sneak past", "Negotiate") — not wildly divergent.
- Call updateWorldState / updatePlotStatus / createPlot tools as needed to mutate the world BEFORE emitting dialogue.
- Use ROLL type for narration describing the process or outcome of skill checks and dice rolls.

BAD example (too divergent): [Option to fly to the moon], [Option to become a farmer]
GOOD example (action-oriented, same scene): [Intimidate the guard], [Bribe the guard], [Find another entrance]
`.trim();
}

/** Pre-generate next steps for all isAiTrigger options. Runs in background. */
async function preGenerateBranches(
  parentStepId: string,
  options: DialogueOption[],
  history: Message[],
  worldSnapshot: Record<string, unknown>
): Promise<void> {
  const aiTriggerOptions = options.filter((o) => o.isAiTrigger);
  if (aiTriggerOptions.length === 0) return;

  const systemPrompt = buildSystemPrompt();

  for (const option of aiTriggerOptions) {
    try {
      const childStepId = `step_${parentStepId}_${option.id}`;
      const promptText = `The player chose: "${option.text}". Generate the narrative response.`;

      const { model } = getModelInfo();
      const result = await generateText({
        model,
        system: systemPrompt,
        messages: [{ role: "user", content: promptText }],
        tools: {
          updateWorldState: createUpdateWorldStateTool(
            new TurnEventEmitter({ write: () => {}, end: () => {} } as unknown as Response, "noop")
          ),
          updatePlotStatus: createUpdatePlotStatusTool(
            new TurnEventEmitter({ write: () => {}, end: () => {} } as unknown as Response, "noop")
          ),
          createPlot: createCreatePlotTool(
            new TurnEventEmitter({ write: () => {}, end: () => {} } as unknown as Response, "noop")
          ),
          dialogue_response: tool({
            description: "Generate the narrative dialogue steps and final player choices.",
            inputSchema: z.object({
              messages: z.array(z.object({
                speaker: z.string(),
                type: z.enum(["YOU", "INNER_VOICE", "CHARACTER", "SYSTEM", "ROLL", "NOTIFICATION"]),
                text: z.string(),
              })),
              options: z.array(z.object({
                text: z.string(),
                id: z.string().optional(),
              })).optional(),
            }),
            execute: async (args: any) => "Dialogue processed."
          })
        },
      });

      // Find the dialogue tool result
      let childOpts: DialogueOption[] = [];
      let stepMessages: Message[] = [];
      
      const dialogueCall = result.toolCalls.find((tc: any) => tc.toolName === 'dialogue_response');
      if (dialogueCall) {
        const args = (dialogueCall as any).args || (dialogueCall as any).input;
        if (args && typeof args === 'object') {
        if (args.messages) {
          stepMessages = args.messages.map((m: any, i: number) => ({
            id: `msg_${childStepId}_${i}`,
            speaker: m.speaker,
            type: m.type as Message["type"],
            text: m.text,
          }));
        }
        if (args.options) {
          childOpts = args.options.map((o: any, i: number) => ({
            id: o.id || `opt_${i}`,
            text: o.text,
            isAiTrigger: true,
          }));
        }
        }
      }

      const finalOpts: DialogueOption[] = childOpts && childOpts.length > 0
        ? childOpts
        : [{ id: "opt_continue", text: "Continue", isAiTrigger: true }];

      saveStep({
        id: childStepId,
        parentStepId,
        parentOptionId: option.id,
        messages: stepMessages,
        options: finalOpts,
        worldSnapshot,
        isGenerated: true,
        isActive: false, // inactive until chosen
      });

      // Add dead-branch replay option
      const deadBranchOpt: DialogueOption = {
        id: `replay_${option.id}`,
        text: `[Replay] ${option.text}`,
        hintBefore: "Dead Branch",
        isAiTrigger: true,
      };

      console.log(`Pre-generated branch: ${childStepId} for option: ${option.id}`);
    } catch (error: unknown) {
      const msg = error instanceof Error ? error.message : String(error);
      console.error(`Failed to pre-generate branch for option ${option.id}:`, msg);
    }
  }
}

export async function generateStreamingResponse(
  userInput: string,
  history: Message[],
  res: Response,
  parentStepId: string | null,
  parentOptionId: string | null
): Promise<void> {
  const systemPrompt = buildSystemPrompt();
  const stepId = `step_${Date.now()}`;
  const events = new TurnEventEmitter(res, stepId);

  // Set up SSE
  res.writeHead(200, {
    "Content-Type": "text/event-stream",
    "Cache-Control": "no-cache",
    Connection: "keep-alive",
    "X-Accel-Buffering": "no",
  });

  events.startStep();

  const historyWindow = 10;
  const promptText = `
## Dialogue History (Last ${historyWindow})
${history.slice(-historyWindow).map((m) => `${m.speaker} (${m.type}): ${m.text}`).join("\n")}

---

## PLAYER ACTION
The player just said/did: "${userInput}"

Generate the narrative response following the output format exactly.
`.trim();

  const { model, name: modelName } = getModelInfo();

  const debugging = new LlmDebugIntegration(
    {
      model: modelName,
      system: systemPrompt,
      prompt: promptText,
      userInput,
      history,
      tools: ["updateWorldState", "updatePlotStatus", "createPlot"],
    },
    undefined,
    "GM"
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
        dialogue_response: tool({
          description: "Generate the narrative dialogue steps and final player choices.",
          inputSchema: z.object({
            messages: z.array(z.object({
              speaker: z.string(),
              type: z.enum(["YOU", "INNER_VOICE", "CHARACTER", "SYSTEM", "ROLL", "NOTIFICATION"]),
              text: z.string(),
            })),
            options: z.array(z.object({
              text: z.string(),
              id: z.string().optional(),
            })).optional(),
          }),
          execute: async (args: any) => "Dialogue streamed."
        })
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

    // Stream text chunks to client
    let toolRawArgs = "";
    let lastRenderedText = "";
    
    let finalMessages: any[] = [];
    let finalOptions: DialogueOption[] = [{ id: "opt_continue", text: "Continue", isAiTrigger: true }];

    for await (const chunk of result.fullStream) {
      if (chunk.type === "text-delta") {
        events.feedToken(chunk.text);
        // If we're getting raw text, try to parse it as we go
        const { messages: parsedMsgs, options: parsedOpts } = parseResponseText(events.getText());
        if (parsedMsgs.length > 0) {
          events.emitStreamingMessages(parsedMsgs);
        }
        if (parsedOpts && parsedOpts.length > 0) {
          events.emitOptions(parsedOpts);
        }
      } else if ((chunk as any).type === "tool-call-delta" && (chunk as any).toolName === "dialogue_response") {
        toolRawArgs += (chunk as any).argsTextDelta;
        try {
          const parsed = parsePartial(toolRawArgs);
          
          if (parsed.messages && Array.isArray(parsed.messages)) {
            const currentMessages: any[] = [];
            
            // Sub-parse each message in case the LLM put headers inside text fields
            for (const m of parsed.messages) {
              const speaker = m.speaker || 'NARRATOR';
              const text = m.text || '';
              
              // If text contains a header (e.g. [LOGIC|INNER_VOICE]), split it
              if (text.trim().startsWith('[') && text.includes(']')) {
                 const subParsed = parseResponseText(text);
                 if (subParsed.messages.length > 0) {
                    currentMessages.push(...subParsed.messages);
                 } else {
                    currentMessages.push({ speaker, type: m.type || 'SYSTEM', text });
                 }
              } else {
                currentMessages.push({
                  speaker,
                  type: m.type || 'SYSTEM',
                  text
                });
              }
            }
            
            finalMessages = currentMessages;
            events.emitStreamingMessages(finalMessages);
          }
          
          if (parsed.options && Array.isArray(parsed.options)) {
            finalOptions = parsed.options.map((o: any, i: number) => ({
              id: o.id || `opt_${i}`,
              text: o.text || "",
              isAiTrigger: true,
            }));
            if (finalOptions.length > 0) {
              events.emitOptions(finalOptions);
            }
          }
        } catch (e) {
          // ignore parsing errors on intermediate JSON
        }
      }
    }

    // Final clean up and emission
    // Use the same sub-parsing logic for final emission
    const finalStructuredMessages: any[] = [];
    for (const m of finalMessages) {
       if (m.text.trim().startsWith('[') && m.text.includes(']')) {
          const sub = parseResponseText(m.text);
          if (sub.messages.length > 0) {
             finalStructuredMessages.push(...sub.messages);
          } else {
             finalStructuredMessages.push(m);
          }
       } else {
          finalStructuredMessages.push(m);
       }
    }
    
    // Fallback to raw text parsing if finalMessages is empty but we have accumulated text
    if (finalStructuredMessages.length === 0 && events.getText().trim()) {
       const rawParsed = parseResponseText(events.getText());
       finalStructuredMessages.push(...rawParsed.messages);
       if (rawParsed.options) finalOptions = rawParsed.options;
    }

    const messages = finalStructuredMessages.map((m) => ({
      speaker: m.speaker || 'NARRATOR',
      type: (m.type as Message["type"]) || 'SYSTEM',
      text: m.text || ''
    }));

    // Emit final parsed result
    events.emitParsed(messages, finalOptions);
    events.emitOptions(finalOptions);

    // Save dialogue step to DB
    const stepMessages = messages.map((m, i) => ({
      id: `msg_${stepId}_${i}`,
      speaker: m.speaker,
      type: m.type as Message["type"],
      text: m.text,
    }));

    saveStep({
      id: stepId,
      parentStepId,
      parentOptionId,
      messages: stepMessages,
      options: finalOptions,
      worldSnapshot: getAllEntities() as unknown as Record<string, unknown>,
      isGenerated: true,
      isActive: true,
    });

    // Save messages to history
    for (let i = 0; i < stepMessages.length; i++) {
      try {
        addMessage(stepMessages[i]);
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : String(err);
        if (!message.includes("UNIQUE constraint failed")) {
          console.error("Failed to save message to history:", message);
        }
      }
    }

    // Deactivate siblings if this is a child step
    if (parentStepId) {
      deactivateSiblingBranches(parentStepId, stepId);
    }

    events.finish();

    // Pre-generate branches in the background
    preGenerateBranches(
      stepId,
      finalOptions,
      [...history, ...stepMessages],
      getAllEntities() as unknown as Record<string, unknown>
    );
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    debugging.onError(error instanceof Error ? error : new Error(message));
    events.emitError(message);
    events.finish();
  }
}

/** Legacy non-streaming response (fallback). */
export async function generateAIResponse(
  userInput: string,
  history: Message[]
): Promise<{ messages: Message[]; options: DialogueOption[] }> {
  const systemPrompt = buildSystemPrompt();
  const historyWindow = 10;
  const promptText = `
## Dialogue History (Last ${historyWindow})
${history.slice(-historyWindow).map((m) => `${m.speaker} (${m.type}): ${m.text}`).join("\n")}

---

## PLAYER ACTION
The player just said/did: "${userInput}"

Generate the narrative response following the output format exactly.
`.trim();

  const { model, name: modelName } = getModelInfo();

  const debugging = new LlmDebugIntegration(
    {
      model: modelName,
      system: systemPrompt,
      prompt: promptText,
      userInput,
      history,
      tools: ["updateWorldState", "updatePlotStatus", "createPlot"],
    },
    undefined,
    "GM"
  );

  try {
    // Use streamText but collect full output
    const result = streamText({
      model,
      system: systemPrompt,
      messages: [{ role: "user", content: promptText }],
      tools: {
        updateWorldState: createUpdateWorldStateTool(
          new TurnEventEmitter({ write: () => {}, end: () => {} } as unknown as Response, "noop")
        ),
        updatePlotStatus: createUpdatePlotStatusTool(
          new TurnEventEmitter({ write: () => {}, end: () => {} } as unknown as Response, "noop")
        ),
        createPlot: createCreatePlotTool(
          new TurnEventEmitter({ write: () => {}, end: () => {} } as unknown as Response, "noop")
        ),
        dialogue_response: tool({
          description: "Generate the narrative dialogue steps and final player choices.",
          inputSchema: z.object({
            messages: z.array(z.object({
              speaker: z.string(),
              type: z.enum(["YOU", "INNER_VOICE", "CHARACTER", "SYSTEM", "ROLL", "NOTIFICATION"]),
              text: z.string(),
            })),
            options: z.array(z.object({
              text: z.string(),
              id: z.string().optional(),
            })).optional(),
          }),
          execute: async (args: any) => "Dialogue processed."
        })
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

    let toolRawArgs = "";
    for await (const chunk of result.fullStream) {
      if ((chunk as any).type === "tool-call-delta" && (chunk as any).toolName === "dialogue_response") {
        toolRawArgs += (chunk as any).argsTextDelta;
      }
    }

    let parsedMsgs: any[] = [];
    let parsedOpts: DialogueOption[] = [];
    try {
      const parsed = parsePartial(toolRawArgs);
      if (parsed.messages) parsedMsgs = parsed.messages;
      if (parsed.options) {
        parsedOpts = parsed.options.map((o: any, i: number) => ({
          id: o.id || `opt_${i}`,
          text: o.text || "",
          isAiTrigger: true,
        }));
      }
    } catch (e) {}

    const messages: Message[] = parsedMsgs.map((m, i) => ({
      id: `ai-${Date.now()}-${i}`,
      speaker: m.speaker || 'SYSTEM',
      type: (m.type as Message["type"]) || 'SYSTEM',
      text: m.text || '',
    }));

    const options: DialogueOption[] = parsedOpts && parsedOpts.length > 0
      ? parsedOpts
      : [{ id: "opt_continue", text: "Continue", isAiTrigger: true }];

    // Save to history
    for (const msg of messages) {
      try {
        addMessage(msg);
      } catch (err: unknown) {
        const errMsg = err instanceof Error ? err.message : String(err);
        if (!errMsg.includes("UNIQUE constraint failed")) {
          console.error("Failed to save message to history:", errMsg);
        }
      }
    }

    return { messages, options };
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    debugging.onError(error instanceof Error ? error : new Error(message));
    throw error;
  }
}
