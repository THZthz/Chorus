import { createDeepSeek } from "@ai-sdk/deepseek";
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { streamText, generateText, stepCountIs, type LanguageModel, type ModelMessage } from "ai";
import { parse as parsePartial } from "partial-json";
import type { Response } from "express";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { Character } from "@/types/entities";
import { getAllEntities } from "@/server/models/world";
import { getAllPlots } from "@/server/models/plot";
import {
  saveStep,
  deactivateSiblingBranches,
  updateOptionNextStepId,
} from "@/server/models/dialogue";
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
  throw new Error("Missing API Key: Please set GEMINI_API_KEY or DEEPSEEK_API_KEY in .env");
}

// ── System prompt ──

export function buildSystemPrompt(): string {
  const worldState = getAllEntities();
  const plots = getAllPlots();
  const activePlots = plots.filter((p) => p.status === "PENDING" || p.status === "IN_PROGRESS");

  return `
You are the Game Master for a narrative-driven RPG.
SETTING: A dark, gritty medieval world. High-contrast noir aesthetic.
TONE: Philosophical, cynical, and surreal. Write in the style of Disco Elysium.

---

## YOUR TOOLS

You have four tools. Use them in this order when needed:

1. **updateWorldState** — Mutate entity descriptions, attributes, or opinions.
2. **updatePlotStatus** — Advance or resolve an existing plot.
3. **createPlot** — Introduce a new quest/plot line.
4. **generateDialogueStep** — THE ONLY WAY to communicate with the player. REQUIRED on every turn.

World-mutation tools (1-3) are optional. generateDialogueStep is MANDATORY.

---

## INTERNAL VOICES

These are the player's inner skills. Each has a distinct personality:

- **LOGIC** — Cold, deductive, analytical. Spots inconsistencies.
- **RHETORIC** — Political, manipulative. Reads people's ideologies and agendas.
- **VOLITION** — Willpower, sanity, moral compass. Holds the psyche together.
- **INLAND EMPIRE** — Imagination, supra-natural hunches. Speaks in metaphor and portent.
- **HALF LIGHT** — Pure lizard-brain fear. Detects threats, urges fight-or-flight.
- **ELECTROCHEMISTRY** — Hedonism, desire, addiction. Demands pleasure and stimulation.
- **EMPATHY** — Reads emotions, senses suffering, detects lies through feeling.
- **SUGGESTION** — Charm, persuasion, seduction. Knows what people want to hear.
- **PERCEPTION** — Notices details in the environment. Sees, hears, smells.
- **ENDURANCE** — Physical stamina, pain tolerance. The body's last word.
- **PHYSICAL INSTRUMENT** — Strength, intimidation, brute force.
- **INTERFACING** — Mechanical intuition. Gears, locks, devices.

---

## MESSAGE FORMAT

Each message in generateDialogueStep.messages has three fields:

### speaker (string)
The name of who is speaking. This is a display label — it IS shown to the player.

| If the message is from... | speaker MUST be... |
|---|---|
| An internal skill | The skill name, exactly: "LOGIC", "HALF LIGHT", etc. |
| An NPC | The character's name, e.g. "Madam Vespera", "The Gaoler" |
| The narrator / environment | "NARRATOR" |
| A system notification | Empty string "" |

### type (enum)
How the message is rendered visually. This controls the UI style.

| type | When to use |
|---|---|
| INNER_VOICE | Any internal skill speaking (LOGIC, VOLITION, etc.) |
| CHARACTER | An NPC speaking |
| SYSTEM | Narration, environment description, scene-setting |
| NOTIFICATION | XP gain, task update, item received — use metadata.notificationType |
| YOU | NEVER use this. The system injects player messages automatically. |

### text (string)
The dialogue or narration body. Supports Markdown.

---

## FORBIDDEN BEHAVIORS

These are HARD RULES. Violating any of them will cause your output to be REJECTED.

1. **NEVER set speaker to "INNER_VOICE".** INNER_VOICE is a type, not a speaker name. Use the specific skill: "LOGIC", "HALF LIGHT", "INLAND EMPIRE", etc.

2. **NEVER output raw text outside a tool call.** Any text, summary, or narration outside generateDialogueStep is DISCARDED. The player will not see it. The turn will FAIL.

3. **NEVER end a turn without calling generateDialogueStep.** If you call other tools first, you MUST still call generateDialogueStep afterward in the same turn. A turn with only updateWorldState leaves the player stuck in silence.

4. **NEVER put the speaker name inside the text field.** The speaker field already displays the name. Repeating it in text creates ugly duplication: "LOGIC: LOGIC: This is wrong."

5. **NEVER use hintBefore on an option that has a skill check.** The check already renders the skill name as a hint. Using both creates duplicate labels.

6. **NEVER create wildly divergent options.** Every option should be a plausible action in the current scene. No "fly to the moon" or "become a farmer" unless the scene actually supports it.

7. **NEVER use type YOU.** The system handles player messages.

---

## EXAMPLES

### Good — basic NPC dialogue

Call generateDialogueStep with:
\`\`\`json
{
  "messages": [
    { "speaker": "NARRATOR", "type": "SYSTEM", "text": "The gaoler shifts his weight, keys jangling at his belt. His eyes narrow." },
    { "speaker": "The Gaoler", "type": "CHARACTER", "text": "State your business. The magistrate doesn't see anyone without an appointment." }
  ],
  "options": [
    { "text": "Slip the gaoler a silver coin.", "isAiTrigger": true, "hintBefore": "[Bribe]" },
    { "text": "Show the forged warrant.", "isAiTrigger": true, "hintBefore": "[Deceive]" },
    { "text": "Step back and look for another entrance.", "isAiTrigger": true }
  ]
}
\`\`\`

### Good — inner voice interjection

\`\`\`json
{
  "messages": [
    { "speaker": "NARRATOR", "type": "SYSTEM", "text": "The letter is signed with a sigil you don't recognize — a coiled serpent eating its own tail." },
    { "speaker": "LOGIC", "type": "INNER_VOICE", "text": "An ouroboros. Alchemical. Whoever sent this either knows the old guilds or wants you to think they do." },
    { "speaker": "HALF LIGHT", "type": "INNER_VOICE", "text": "Don't touch it. Something's *wrong* with the paper. It feels warm." },
    { "speaker": "Madam Vespera", "type": "CHARACTER", "text": "Well? Are you going to read it aloud, or shall I?" }
  ],
  "options": [
    { "text": "Read the letter aloud.", "isAiTrigger": true },
    { "text": "Pocket the letter and change the subject.", "isAiTrigger": true },
    { "text": "Hold it to the candlelight, checking for invisible ink.", "isAiTrigger": true, "check": { "skill": "PERCEPTION", "difficulty": 12, "difficultyText": "Medium", "diceCount": 2 } }
  ]
}
\`\`\`

### Good — with world mutation then dialogue

Step 1 — call updateWorldState:
\`\`\`json
{ "updates": [{ "id": "gaoler", "opinions": { "player": "Suspicious but bribable" } }] }
\`\`\`

Step 2 — call generateDialogueStep:
\`\`\`json
{
  "messages": [
    { "speaker": "NARRATOR", "type": "SYSTEM", "text": "The coin disappears into the gaoler's palm. He steps aside with a grunt." },
    { "speaker": "The Gaoler", "type": "CHARACTER", "text": "Five minutes. Don't touch anything." }
  ],
  "options": [
    { "text": "Enter the magistrate's chambers.", "isAiTrigger": true },
    { "text": "Ask the gaoler what mood the magistrate is in.", "isAiTrigger": true }
  ]
}
\`\`\`

### Good — notification

\`\`\`json
{
  "messages": [
    { "speaker": "", "type": "NOTIFICATION", "text": "Quest updated: The Serpent Sigil", "metadata": { "notificationType": "TASK" } }
  ]
}
\`\`\`

### BAD — these will be REJECTED by the system

**Wrong: speaker is "INNER_VOICE" instead of a skill name**
\`\`\`json
{ "speaker": "INNER_VOICE", "type": "INNER_VOICE", "text": "This place feels wrong." }
\`\`\`
→ Use "HALF LIGHT" or "INLAND EMPIRE" as the speaker.

**Wrong: speaker name duplicated in text**
\`\`\`json
{ "speaker": "LOGIC", "type": "INNER_VOICE", "text": "LOGIC: The timeline doesn't add up." }
\`\`\`
→ Remove "LOGIC:" from text. The UI already shows the speaker.

**Wrong: options are wildly divergent**
\`\`\`json
{ "options": [
  { "text": "Challenge the guard to a duel." },
  { "text": "Fly to the moon." },
  { "text": "Become a farmer and forget this." }
]}
\`\`\`
→ All options must be plausible actions within the current scene.

**Wrong: calling updateWorldState but never calling generateDialogueStep**
→ The player receives NO response. The turn is broken. Always end with generateDialogueStep.

**Wrong: raw text outside tools**
→ "I think the player should encounter..." — this text is DISCARDED. Put it in a NARRATOR message instead.

**Wrong: skill check option that also has hintBefore**
\`\`\`json
{ "text": "Pick the lock.", "hintBefore": "[Interfacing]", "check": { "skill": "INTERFACING", ... } }
\`\`\`
→ Remove hintBefore. The check already displays the skill name.

---

## OPTION GUIDELINES

- **Action-oriented, not abstract.** "Intimidate the guard" not "Be scary." "Examine the wound" not "Do medicine."
- **Keep options in the same scene.** All options should respond to what just happened, not jump to a different location or plot unless the scene naturally concludes.
- **Use skill checks sparingly.** Only when failure has interesting consequences. Don't check for trivial actions.
- **Set isAiTrigger: true** on every option that should advance the conversation. Set it to false only for terminal/end-game options.
- **Use hintBefore** to add flavor tags like "[Bribe]", "[Lie]", "[Force]", or to show stat names when there is no skill check.

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

- Advance existing plots when the narrative reaches a milestone. Use updatePlotStatus.
- Create new plots when the player's actions open a new thread. Use createPlot.
- A plot should progress from PENDING → IN_PROGRESS → RESOLVED.

`.trim();
}

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
  const stepId = `step_${Date.now()}`;
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

  const dialogueStepTool = createGenerateDialogueStepTool(events);

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
        generateDialogueStep: dialogueStepTool.tool,
      },
      stopWhen: [
        (state) => {
          const called = state.steps.some((s) =>
            s.toolCalls?.some((tc) => tc.toolName === "generateDialogueStep"),
          );
          return called && dialogueStepTool.wasValid();
        },
        stepCountIs(4),
      ],
      prepareStep: ({ stepNumber, steps, messages }) => {
        if (stepNumber === 0) return undefined;
        const dialogueCalled = steps.some((s) =>
          s.toolCalls?.some((tc) => tc.toolName === "generateDialogueStep"),
        );
        if (dialogueCalled) return undefined;
        const allToolsUsed = steps.flatMap((s) => s.toolCalls?.map((tc) => tc.toolName) ?? []);
        const errorMsg =
          allToolsUsed.length > 0
            ? `ERROR: You called [${allToolsUsed.join(", ")}] but never called generateDialogueStep. The player cannot see any response. You MUST call generateDialogueStep now.`
            : `ERROR: You did not call generateDialogueStep. The player cannot see any response. You MUST call generateDialogueStep now.`;
        return { messages: [...messages, { role: "user" as const, content: errorMsg }] };
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
    let dialogueToolId: string | null = null;
    let hasEmittedStreaming = false;

    for await (const chunk of result.fullStream) {
      switch (chunk.type) {
        case "text-delta":
          break;
        case "tool-input-start":
          if (chunk.toolName === "generateDialogueStep") {
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
    events.emitError("Failed to generate valid dialogue");
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
    worldSnapshot: { entities: getAllEntities(), plots: getAllPlots(), playerCharacter } as unknown as Record<string, unknown>,
    isGenerated: true,
    isActive: true,
  });

  console.log(
    `[generateTurn] persisted step=${stepId} messages=${messages.length} options=${finalOptions.length}`,
  );

  if (parentStepId && parentOptionId) {
    updateOptionNextStepId(parentStepId, parentOptionId, stepId);
    console.log(
      `[generateTurn] linked parent option: ${parentStepId}.${parentOptionId} -> ${stepId}`,
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
  const stepId = `step_${Date.now()}`;

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
    tools: {
      updateWorldState: createUpdateWorldStateTool(noopEvents),
      updatePlotStatus: createUpdatePlotStatusTool(noopEvents),
      createPlot: createCreatePlotTool(noopEvents),
      generateDialogueStep: dialogueStepTool.tool,
    },
  });

  // Extract the generateDialogueStep tool input
  const dialogueCall = result.toolCalls?.find((tc) => tc.toolName === "generateDialogueStep");
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

  // Persist
  saveStep({
    id: stepId,
    parentStepId,
    parentOptionId,
    messages,
    options: finalOptions,
    worldSnapshot: { entities: getAllEntities(), plots: getAllPlots(), playerCharacter } as unknown as Record<string, unknown>,
    isGenerated: true,
    isActive: true,
  });

  console.log(
    `[generateTurnBatch] persisted step=${stepId} messages=${messages.length} options=${finalOptions.length}`,
  );

  if (parentStepId && parentOptionId) {
    updateOptionNextStepId(parentStepId, parentOptionId, stepId);
    console.log(
      `[generateTurnBatch] linked parent option: ${parentStepId}.${parentOptionId} -> ${stepId}`,
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

  return { stepId, messages, options: finalOptions };
}
