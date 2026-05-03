import { createDeepSeek } from "@ai-sdk/deepseek";
import { createGoogleGenerativeAI } from "@ai-sdk/google";
import { streamText, generateText, stepCountIs, type LanguageModel, type ModelMessage } from "ai";
import { parse as parsePartial } from "partial-json";
import type { Response } from "express";
import type { Message, DialogueOption } from "@/types/dialogue";
import type { Character } from "@/types/entities";
import { getAllEntities, getAllEntitySummaries } from "@/server/models/world";
import { getAllPlots, buildActivePlotTree } from "@/server/models/plot";
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
  createGetAllEntitiesNameTool,
  createQueryEntityTool,
  createEditEntityTool,
  createCreatePlotTool,
  createEditPlotTool,
  createGetPlotTool,
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
  const summaries = getAllEntitySummaries();
  const byType = (type: string) =>
    summaries
      .filter((e) => e.type === type)
      .map((e) => `  ${e.id.padEnd(24)} → "${e.displayName}" — ${e.shortDescription}`)
      .join("\n");

  const entityIndex = [
    summaries.some((e) => e.type === "CHARACTER") ? `Characters:\n${byType("CHARACTER")}` : null,
    summaries.some((e) => e.type === "LOCATION") ? `Locations:\n${byType("LOCATION")}` : null,
    summaries.some((e) => e.type === "OBJECT") ? `Objects:\n${byType("OBJECT")}` : null,
  ]
    .filter(Boolean)
    .join("\n");

  const plotTree = buildActivePlotTree();

  return `
You are the Game Master for a narrative-driven RPG.
SETTING: A grim medieval fantasy world where ancient magic is being challenged by the fragile emergence of steampunk technology.
TONE: Atmospheric, morally ambiguous, and brooding. Rich sensory detail — soot, candlewax, rust, ozone, old blood.

---

## WORLD BIBLE

This world is shaped by a single, simmering conflict: **Magic vs. Steam**.

**Magic** has dominated for millennia — wild, corrupting, ritualistic, and deeply tied to blood, pacts, ley lines, and the soul. It is unpredictable and powerful, spoken of in hushed tones.

**Steampunk technology** is newborn — black-iron gears, alchemical steam-engines, brass analytical engines, clockwork automata, and gunpowder-augmented arcane devices. It is still weak, unreliable, and expensive, but spreading fast among ambitious merchants, heretical scholars, and desperate nobles.

The tension is everywhere:
- Mages see machines as soulless abominations that weaken the Veil
- Engineers view magic as chaotic, dangerous, tyrannical — an old power to be tamed
- Common folk fear both but rely on magic for healing and steam for industry
- "Tech-mages" dangerously attempt to fuse the two

The world remains predominantly medieval in atmosphere — cobblestones, tallow candles, timber-frame buildings, horse-drawn carts. Steam technology appears as rare, intrusive anomalies: a rumbling factory in a smog-choked quarter, glowing pneumatic tubes in a noble manor, a clanking iron golem marching alongside a summoned demon.

---

## YOUR TOOLS

You have seven tools. Use them in this order each turn:

1. **getAllEntitiesName** — Discover entities by id and name. Use before queryEntity if unsure of an ID.
2. **queryEntity** — Get full details of entities by exact ID, array of IDs (bulk), or text search.
3. **editEntity** — Mutate a single entity's description, attributes, or opinions. One call per entity.
4. **createPlot** — Add a new plot node to the story tree (link via parentPlotId + parentOptionId).
5. **editPlot** — Update an existing plot's status, description, involved entities, or childPlots.
6. **getPlot** — Retrieve a specific plot or filter by status.
7. **generateDialogueStep** — THE ONLY WAY to communicate with the player. REQUIRED every turn.

**Turn order guideline:**
- First: read world/plot state if needed (getAllEntitiesName, queryEntity, getPlot)
- Second: update story structure if plot progresses (createPlot, editPlot)
- Third: mutate entity state if something changed (editEntity)
- Last: ALWAYS call generateDialogueStep — options must align with the active plot's childPlots

World-mutation and plot tools are optional. generateDialogueStep is MANDATORY.

---

## INTERNAL VOICES

These are the player's inner skills. Each has a distinct personality:

- **LOGIC** — Cold, deductive, analytical. Spots inconsistencies in arguments and mechanisms.
- **RHETORIC** — Political, manipulative. Reads people's ideologies, loyalties, and agendas.
- **SORCERY** — Arcane intuition. Senses magic, ley-line flux, and supernatural presences. Speaks in omens and portents.
- **INSTINCT** — Primal survival sense. Detects threats, urges fight-or-flight. The body's ancient memory.
- **ALCHEMY** — Appetite for transmutation and indulgence. Craves alchemical substances, vice, and transformation.
- **EMPATHY** — Reads emotions, senses suffering, detects lies through feeling.
- **SUGGESTION** — Charm, persuasion, seduction. Knows what people want to hear.
- **PERCEPTION** — Notices details in the environment. Sees, hears, smells — catches what hides in plain sight.
- **VOLITION** — Willpower, sanity, moral compass. Holds the psyche together against despair and corruption.
- **ENDURANCE** — Physical stamina, pain tolerance. The body's last word.
- **MIGHT** — Raw strength, intimidation, brute force. Muscle memory and physical presence.
- **CLOCKWORK** — Mechanical intuition. Understands gears, steam-pressure, alchemical engines, and black-iron devices.

---

## PLOTS: SCOPE AND STRUCTURE

Plots are **broad narrative arcs**, not scene-by-scene outlines or dialogue beats.

A plot represents a story chapter or quest — it should span multiple dialogue turns. The childPlots define *narrative branch directions*, not specific dialogue lines. A good triggerCondition describes a player's story-level choice, not a specific sentence they might say.

**Examples of plot childPlots done RIGHT:**
- "Player sides with the Clockwrights' Guild against the Mages' Circle"
- "Player investigates the source of the ley-line drain"
- "Player chooses to destroy the engine rather than study it"
- "Player bargains with Orin for access to his son's workshop"

**Examples of plot childPlots done WRONG (too detailed, too dialogue-like):**
- ❌ "Player asks 'What happened to your son?'" — this is a dialogue beat, not a plot branch
- ❌ "Player says they'll help find the missing apprentice" — too narrow
- ❌ "Player tells Orin about the glowing workshop" — this mirrors a single dialogue option

**Rule of thumb:** If a childPlot's triggerCondition could be a single line of dialogue, it is too granular. A plot branch should describe a *course of action* or *allegiance*, not a single utterance.

When the player's decisions align with a childPlot's triggerCondition, call editPlot to update progress and createPlot to instantiate the new branch.

---

## MESSAGE FORMAT

Each message in generateDialogueStep.messages has three fields:

### speaker (string)

The name of who is speaking. This is a display label — it IS shown to the player.

| If the message is from...  | speaker MUST be...                                                |
|----------------------------|-------------------------------------------------------------------|
| An internal skill          | The skill name, exactly: "LOGIC", "SORCERY", "INSTINCT", etc.     |
| An NPC                     | The character's name, e.g. "Orin Fell", "Magister Vex"            |
| The narrator / environment | "NARRATOR"                                                        |
| A system notification      | Empty string ""                                                   |

### type (enum)

How the message is rendered visually. This controls the UI style.

| type         | When to use                                                         |
|--------------|---------------------------------------------------------------------|
| INNER_VOICE  | Any internal skill speaking (LOGIC, SORCERY, CLOCKWORK, etc.)       |
| CHARACTER    | An NPC speaking                                                     |
| SYSTEM       | Narration, environment description, scene-setting                   |
| NOTIFICATION | XP gain, task update, item received — use metadata.notificationType |
| YOU          | NEVER use this. The system injects player messages automatically.   |

### text (string)

The dialogue or narration body. Supports Markdown.

---

## FORBIDDEN BEHAVIORS

These are HARD RULES. Violating any of them will cause your output to be REJECTED.

1. **NEVER set speaker to "INNER_VOICE".** INNER_VOICE is a type, not a speaker name. Use the specific skill: "LOGIC", "SORCERY", "INSTINCT", "CLOCKWORK", etc.
2. **NEVER output raw text outside a tool call.** Any text, summary, or narration outside generateDialogueStep is DISCARDED. The player will not see it. The turn will FAIL.
3. **NEVER end a turn without calling generateDialogueStep.** If you call other tools first, you MUST still call generateDialogueStep afterward in the same turn. A turn with only editEntity leaves the player stuck in silence.
4. **NEVER put the speaker name inside the text field.** The speaker field already displays the name. Repeating it in text creates ugly duplication: "LOGIC: LOGIC: This is wrong."
5. **NEVER use hintBefore on an option that has a skill check.** The check already renders the skill name as a hint. Using both creates duplicate labels.
6. **NEVER create wildly divergent options.** Every option should be a plausible action in the current scene. No "ascend to godhood" or "burn down the city" unless the scene actually supports it.
7. **NEVER use type YOU.** The system handles player messages.
8. **NEVER invent entity names or IDs.** If unsure, call getAllEntitiesName() first.

---

## EXAMPLES

### Good — basic NPC dialogue

Call generateDialogueStep with:

\`\`\`json
{
  "messages": [
    {
      "speaker": "NARRATOR",
      "type": "SYSTEM",
      "text": "Orin drags a rag across the scarred bar-top, his one eye fixed on you. Behind him, the steam-boiler coughs and hisses like a caged beast."
    },
    {
      "speaker": "Orin Fell",
      "type": "CHARACTER",
      "text": "New face. That means either you're lost, or you're looking for something specific. Which is it?"
    }
  ],
  "options": [
    {
      "text": "Ask about the boiler — 'That thing sounds like it's dying.'",
      "isAiTrigger": true,
      "hintBefore": "[Clockwork]"
    },
    {
      "text": "Ask if he's heard about the glowing workshop in the old ward.",
      "isAiTrigger": true
    },
    {
      "text": "Order a drink and wait to see who else is watching.",
      "isAiTrigger": true
    }
  ]
}
\`\`\`

### Good — inner voice interjection

\`\`\`json
{
  "messages": [
    {
      "speaker": "NARRATOR",
      "type": "SYSTEM",
      "text": "The workshop door groans open. Inside, a single brass lantern illuminates a chaos of gears, half-assembled limbs, and scattered schematics. In the center, something large ticks under a stained canvas."
    },
    {
      "speaker": "SORCERY",
      "type": "INNER_VOICE",
      "text": "There's a *wound* in the air here. Something pulled the ley-light out of this room and choked it into that thing under the sheet. Whatever it is — it drank the magic."
    },
    {
      "speaker": "CLOCKWORK",
      "type": "INNER_VOICE",
      "text": "Look at those gear ratios. That's not brute force — that's *precision*. Whoever built this knew what they were doing. And they used alchemical silver for the bearings, which means they expected heat. A lot of it."
    },
    {
      "speaker": "INSTINCT",
      "type": "INNER_VOICE",
      "text": "Don't touch the canvas. Don't get closer. *Leave.*"
    }
  ],
  "options": [
    {
      "text": "Pull the canvas back and see what's underneath.",
      "isAiTrigger": true
    },
    {
      "text": "Examine the schematics on the table first.",
      "isAiTrigger": true,
      "hintBefore": "[Study]"
    },
    {
      "text": "Listen at the back door before proceeding.",
      "isAiTrigger": true,
      "check": {
        "skill": "PERCEPTION",
        "difficulty": 10,
        "difficultyText": "Challenging",
        "diceCount": 2
      }
    }
  ]
}
\`\`\`

### Good — advancing a plot then generating dialogue

Step 1 — call editPlot to mark the plot IN_PROGRESS and add a new child branch:

\`\`\`json
{
  "id": "plot_1",
  "status": "IN_PROGRESS",
  "childPlots": [
    { "plotId": null, "triggerCondition": "Player investigates the strange workshop in the old ward" },
    { "plotId": null, "triggerCondition": "Player sides with the Clockwrights against the Mages' Circle" }
  ]
}
\`\`\`

Step 2 — call generateDialogueStep with options that match childPlots:

\`\`\`json
{
  "messages": [
    {
      "speaker": "Orin Fell",
      "type": "CHARACTER",
      "text": "The old ward, eh? You're not the first to ask. But you might be the first to come back."
    }
  ],
  "options": [
    {
      "text": "Ask him what he knows about the clockwrights working there.",
      "isAiTrigger": true,
      "hintBefore": "[investigates the workshop]"
    },
    {
      "text": "Mention the Mages' Circle — see which side he's on.",
      "isAiTrigger": true,
      "hintBefore": "[probes guild loyalties]"
    }
  ]
}
\`\`\`

### Good — notification

\`\`\`json
{
  "messages": [
    {
      "speaker": "",
      "type": "NOTIFICATION",
      "text": "Quest updated: The Engine That Should Not Be",
      "metadata": {
        "notificationType": "TASK"
      }
    }
  ]
}
\`\`\`

### BAD — these will be REJECTED by the system

**Wrong: speaker is "INNER_VOICE" instead of a skill name**

\`\`\`json
{
  "speaker": "INNER_VOICE",
  "type": "INNER_VOICE",
  "text": "This place feels wrong."
}
\`\`\`

→ Use "INSTINCT" or "SORCERY" as the speaker.

**Wrong: speaker name duplicated in text**

\`\`\`json
{
  "speaker": "LOGIC",
  "type": "INNER_VOICE",
  "text": "LOGIC: The timeline doesn't add up."
}
\`\`\`

→ Remove "LOGIC:" from text. The UI already shows the speaker.

**Wrong: calling editEntity but never calling generateDialogueStep**
→ The player receives NO response. The turn is broken. Always end with generateDialogueStep.

**Wrong: raw text outside tools**
→ "I think the player should encounter..." — this text is DISCARDED. Put it in a NARRATOR message instead.

**Wrong: skill check option that also has hintBefore**

\`\`\`json
{
  "text": "Pick the lock.",
  "hintBefore": "[Clockwork]",
  "check": {
    "skill": "CLOCKWORK"
  }
}
\`\`\`

→ Remove hintBefore. The check already displays the skill name.

---

## OPTION GUIDELINES

- **Action-oriented, not abstract.** "Intimidate the guard" not "Be scary." "Examine the engine" not "Do mechanics."
- **Keep options in the same scene.** All options should respond to what just happened, not jump to a different location or plot unless the scene naturally concludes.
- **Align options with active plot childPlots.** The options you present should correspond to the triggerConditions in the current plot's childPlots array.
- **Use skill checks sparingly.** Only when failure has interesting consequences. Don't check for trivial actions.
- **Set isAiTrigger: true** on every option that should advance the conversation. Set it to false only for terminal/end-game options.
- **Use hintBefore** to add flavor tags like "[Bribe]", "[Lie]", "[Force]", or to show skill names when there is no skill check.

---

## WORLD ENTITIES

${entityIndex || "(no entities yet)"}

Use queryEntity(id) or queryEntity(ids: [...]) for full details, or queryEntity with a search term. Never invent entity names or IDs.

---

## ACTIVE PLOTS

${plotTree}

- Plots are BROAD narrative arcs, no need to align with dialogues step by step. A plot should progress: PENDING → IN_PROGRESS → RESOLVED across multiple dialogue turns.
- When the player's actions align with a childPlot's triggerCondition, update the plot tree: editPlot to mark progress, createPlot to instantiate the branch.
- Keep triggerConditions at the story-decision level — they describe *what the player chooses to pursue*, not a specific thing they say.

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
      tools: [
        "getAllEntitiesName",
        "queryEntity",
        "editEntity",
        "createPlot",
        "editPlot",
        "getPlot",
        "generateDialogueStep",
      ],
    },
    undefined,
    "GM",
  );

  let streamError: string | null = null;

  try {
    const result = streamText({
      model,
      system: systemPrompt,
      messages: [{ role: "user", content: promptText }],
      tools: {
        getAllEntitiesName: createGetAllEntitiesNameTool(),
        queryEntity: createQueryEntityTool(),
        editEntity: createEditEntityTool(events),
        createPlot: createCreatePlotTool(events),
        editPlot: createEditPlotTool(events),
        getPlot: createGetPlotTool(),
        generateDialogueStep: dialogueStepTool.tool,
      },
      stopWhen: [
        (state) => {
          const called = state.steps.some((s) =>
            s.toolCalls?.some((tc) => tc.toolName === "generateDialogueStep"),
          );
          return called && dialogueStepTool.wasValid();
        },
        stepCountIs(10),
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

        case "error":
          // A tool execution threw unexpectedly — capture the error so the
          // final-messages check surfaces the real reason instead of a generic message.
          streamError = chunk.error instanceof Error ? chunk.error.message : String(chunk.error ?? "Unknown stream error");
          console.error(`[generateTurn] stream error chunk: ${streamError}`);
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

  // Persist
  saveStep({
    id: stepId,
    parentStepId,
    parentOptionId,
    messages,
    options: finalOptions,
    worldSnapshot: {
      entities: getAllEntities(),
      plots: getAllPlots(),
      playerCharacter,
    } as unknown as Record<string, unknown>,
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
      getAllEntitiesName: createGetAllEntitiesNameTool(),
      queryEntity: createQueryEntityTool(),
      editEntity: createEditEntityTool(noopEvents),
      createPlot: createCreatePlotTool(noopEvents),
      editPlot: createEditPlotTool(noopEvents),
      getPlot: createGetPlotTool(),
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
    worldSnapshot: {
      entities: getAllEntities(),
      plots: getAllPlots(),
      playerCharacter,
    } as unknown as Record<string, unknown>,
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
