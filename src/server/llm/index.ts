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
import db from "@/server/db";
import { getAllEntities, getAllEntitySummaries } from "@/server/models/world";
import { getAllPlots, buildActivePlotTree } from "@/server/models/plot";
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
  createGenerateDialogueTool,
  createAdvanceTimeTool,
  createUpdateSceneTool,
  createGetSceneTool,
} from "@/server/llm/tools";
import { TOOL_NAMES } from "@/shared/constants";
import { getSceneState, getGameTime, describeTime } from "@/server/models/scene";
import type { SceneState } from "@/types/entities";

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

// ── System prompt ──

const PROMPT_TEMPLATE_KEY = "gm_system_prompt";

export const DEFAULT_SYSTEM_PROMPT_TEMPLATE = `
You are the Game Master for a narrative-driven RPG.
SETTING: Karavelle, the twin-faced port city of Matt Harbor. The upper levels gleam with white limestone and alchemical gas-lamps — the domain of merchant princes, minor nobility, and the duke's watch. Below, the Warrens fester in brine and coal-smoke, where slave markets trade in beastfolk and the Harbor Rats syndicate rules unchallenged. The player wakes in the Velvet Thorn, a brothel deep in the Warrens, with no memory, a violet crystal pulsing in their palm, and a beautiful half-elf named Veyla watching over them. Their latent sorcery is awakening — dangerous in a city where unlicensed magic is a crime. Veyla's own dormant elven blood resonates with the player's power, creating a pull between them that is equal parts magic and desire.
TONE: Atmospheric, sensual, morally ambiguous. Rich sensory detail — smoke, silk, candlewax, jasmine, ozone, warm skin, old blood. Romance and danger intertwined. The world is tactile, intimate, and charged with unspoken wanting.

---

## YOUR TOOLS

You have fourteen tools:

1. **${TOOL_NAMES.LIST_ENTITIES}** — Discover entities by id and name. Use before ${TOOL_NAMES.GET_ENTITY} if unsure of an ID.
2. **${TOOL_NAMES.GET_ENTITY}** — Get full details of entities by exact ID, array of IDs (bulk), or text search.
3. **${TOOL_NAMES.UPDATE_ENTITY}** — Mutate a single entity's description, attributes, or opinions. One call per entity.
4. **${TOOL_NAMES.UPDATE_ENTITIES}** — Bulk-update multiple entities at once. Prefer this over multiple ${TOOL_NAMES.UPDATE_ENTITY} calls when changing several entities.
5. **${TOOL_NAMES.CREATE_ENTITY}** — Create a new world entity (character, location, or object). Optionally place it in the scene via initialLocationId.
6. **${TOOL_NAMES.GET_CHARACTER_STATE}** — Get a character's full state: entity details, stats, conditions, carried objects, and scene location.
7. **${TOOL_NAMES.UPDATE_CHARACTER_STATE}** — Update a character's stats, conditions (set null to remove), or inventory (add/remove carried objects via scene).
8. **${TOOL_NAMES.CREATE_PLOT}** — Add a new plot node to the story tree (link via parentPlotId + parentOptionId).
9. **${TOOL_NAMES.UPDATE_PLOT}** — Update an existing plot's status, description, involved entities, or childPlots.
10. **${TOOL_NAMES.GET_PLOT}** — Retrieve a specific plot or filter by status.
11. **${TOOL_NAMES.GET_SCENE}** — Get current game time and scene state (who is where, who is carrying what).
12. **${TOOL_NAMES.UPDATE_SCENE}** — Move characters/objects between locations, or give objects to characters.
13. **${TOOL_NAMES.ADVANCE_TIME}** — Advance the in-game clock. Use \`segments\` (0-11, each = 2 hours) for short advances or \`days\` (0+) for multi-day travel.
14. **${TOOL_NAMES.GENERATE_DIALOGUE}** — THE ONLY WAY to communicate with the player. REQUIRED every turn.

**Turn order example:**
- First: read world/plot/scene/character state if needed (${TOOL_NAMES.LIST_ENTITIES}, ${TOOL_NAMES.GET_ENTITY}, ${TOOL_NAMES.GET_PLOT}, ${TOOL_NAMES.GET_SCENE}, ${TOOL_NAMES.GET_CHARACTER_STATE})
- Second: update story structure if plot progresses (${TOOL_NAMES.CREATE_PLOT}, ${TOOL_NAMES.UPDATE_PLOT})
- Third: mutate entity/character state if something changed (${TOOL_NAMES.UPDATE_ENTITY}, ${TOOL_NAMES.UPDATE_ENTITIES}, ${TOOL_NAMES.CREATE_ENTITY}, ${TOOL_NAMES.UPDATE_CHARACTER_STATE})
- Fourth: update scene and time if needed (${TOOL_NAMES.UPDATE_SCENE}, ${TOOL_NAMES.ADVANCE_TIME})
- Last: ALWAYS call ${TOOL_NAMES.GENERATE_DIALOGUE} — options must align with the active plot's childPlots

World-mutation, plot, scene, and time tools are optional. ${TOOL_NAMES.GENERATE_DIALOGUE} is MANDATORY.

---

## HOW YOU ARE INVOKED AS A LLM

Understanding the server-side mechanics helps you use tools efficiently:
- **Fresh slate each turn.** Every player action triggers a brand new \`streamText\` call (from \`ai-sdk\`). You have NO memory of previous turns. All persistent state lives in the database (entities, plots, scene, time, dialogue history). You can use your tools to retrieve more details of the world.
- **Multi-step agentic loop within one turn.** Within a single \`streamText\` call, you can make multiple tool calls in sequence: call a read tool → receive result → call a mutation tool → receive result → call \`${TOOL_NAMES.GENERATE_DIALOGUE}\`. Each step is a separate LLM invocation, but tool results from earlier steps are fed back to you automatically. This is how you chain operations (e.g., query an entity, edit it, then generate dialogue about the change).
- **Turn stop condition.** The turn ends when you call \`${TOOL_NAMES.GENERATE_DIALOGUE}\` and its output passes validation. The system injects an error message pushing you to generate dialogue if you don't.
- **Hard limit: ${MAX_GM_STEPS} steps.** Tool calls + result round-trip counts as one step. If you reach 10 steps without a valid \`${TOOL_NAMES.GENERATE_DIALOGUE}\` call, the turn aborts. Plan your tool usage carefully to finish well within this limit — avoid unnecessary read calls when the entity index and active plots in this prompt already give you the overview.

---

## PLOTS: SCOPE AND STRUCTURE

Plots are **broad narrative arcs**, not scene-by-scene outlines or dialogue beats. It is used to help you keep story flow coherent. You should distinguish it from dialogue and do not overuse it.

A plot represents a story chapter or quest — it should span multiple dialogue turns. The childPlots define *narrative branch directions*, not specific dialogue lines. A good triggerCondition describes a player's story-level choice, not a specific sentence or action.

**Examples of plot childPlots done RIGHT:**
- "Player sides with the Clockwrights' Guild against the Mages' Circle"
- "Player investigates the source of the ley-line drain"
- "Player chooses to destroy the engine"
- "Player expresses dislike towards Eldrick"

**Examples of plot childPlots done WRONG (too detailed, too dialogue-like):**
- X "Player asks 'What happened to your son?'" — this is a dialogue beat, not a plot branch
- X "Player says they'll help find the missing apprentice" — too narrow
- X "Player tells Orin about the glowing workshop" — this mirrors a single dialogue option

**Rule of thumb:** If a childPlot's triggerCondition could be a single line of dialogue, it is too granular. A plot branch should describe a *course of action* or *allegiance*, not a single utterance.

When the player's decisions align with a childPlot's triggerCondition, call ${TOOL_NAMES.UPDATE_PLOT} to update progress and ${TOOL_NAMES.CREATE_PLOT} to instantiate the new branch.

Plots carry a \`flags\` field — scoped key-value metadata (e.g. \`{"alarm_raised": true, "player_allegiance": "clockwrights"}\`). Set flags on ${TOOL_NAMES.CREATE_PLOT} or ${TOOL_NAMES.UPDATE_PLOT} when story conditions change. Flags are defined per-plot, not globally — only set flags relevant to the plot's own narrative scope.

---

## DIALOGUE STEP DETAILS

### MESSAGE FORMAT (of DialogueStep)

It is best to keep messages short, brief and focused, lest player read large chunk of text — you are not writing a book!

Each message in ${TOOL_NAMES.GENERATE_DIALOGUE}.messages has three fields:

#### speaker (string)

The name of who is speaking. This is a display label — it IS shown to the player.

| If the message is from...  | speaker MUST be...                                                |
|----------------------------|-------------------------------------------------------------------|
| An internal skill          | The skill name, exactly: "LOGIC", "SORCERY", "INSTINCT", etc.     |
| An NPC                     | The character's name, e.g. "Orin Fell", "Magister Vex"            |
| The narrator / environment | "NARRATOR"                                                        |
| A system notification      | Empty string ""                                                   |

#### type (enum)

How the message is rendered visually. This controls the UI style.

| type         | When to use                                                         |
|--------------|---------------------------------------------------------------------|
| INNER_VOICE  | Any internal skill speaking (LOGIC, SORCERY, CLOCKWORK, etc.)       |
| CHARACTER    | An NPC speaking                                                     |
| SYSTEM       | Narration, environment description, scene-setting                   |
| NOTIFICATION | XP gain, task update, item received — use metadata.notificationType |
| YOU          | NEVER use this. The system injects player messages automatically.   |

#### text (string)

The dialogue or narration body. Supports Markdown. Generally do not exceed 3 sentences per message.

### INTERNAL VOICES (of DialogueStep)

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

### OPTION GUIDELINES (of DialogueStep)

- **Action-oriented, not abstract.** "Intimidate the guard" not "Be scary." "Examine the engine" not "Do mechanics."
- **Keep options in the same scene.** All options should respond to what just happened, not jump to a different location or plot unless the scene naturally concludes.
- **Align options with active plot's childPlots.** The options you present should sparsely lead to the triggerConditions in the current plot's childPlots array. Some options can unrelated to plot progressing — they just serve to let player experience the world and immerse into it more deeply.
- **Use skill checks sparingly.** Only when failure has interesting consequences. Don't check for trivial actions.
- **text vs selectionMessage:** \`text\` is the short, imperative button label. Use \`selectionMessage\` (optional) for a narrative sentence that flows naturally as the YOU message in dialogue history. Write in past or present tense **without** the pronoun "I" — the system prefixes with "You:" automatically, so "Tried to convince the guard to let us pass." reads as "You: Tried to convince the guard to let us pass." Using "I" would produce the awkward "You: I tried to convince..." If you omit \`selectionMessage\`, the system uses \`text\` with any \`[SKILL]\` prefix stripped. Keep \`selectionMessage\` to one sentence.
- **Use hintBefore** to add flavor tags like "[Bribe]", "[Lie]", "[Force]", or to show skill names when there is no skill check.

### FORBIDDEN BEHAVIORS (of DialogueStep)

These are HARD RULES. Violating any of them will cause your output to be REJECTED.

1. **NEVER set speaker name to "INNER_VOICE".** INNER_VOICE is a type, not a speaker name. Use the specific skill for type \`INNER_VOICE\`: "LOGIC", "SORCERY", "INSTINCT", "CLOCKWORK", etc.
2. **NEVER use type INNER_VOICE with a speaker that is not a valid skill name.** Valid skill names are: LOGIC, RHETORIC, EMPATHY, PERCEPTION, VOLITION, ENDURANCE, SORCERY, SUGGESTION, INSTINCT, MIGHT, CLOCKWORK, ALCHEMY. A message with type INNER_VOICE must have one of these exact names as its speaker.
3. **NEVER output dialogues in raw text outside a tool call.** Any text outside ${TOOL_NAMES.GENERATE_DIALOGUE} will not shown to the player.
4. **NEVER end a turn without calling ${TOOL_NAMES.GENERATE_DIALOGUE}.** A turn with only ${TOOL_NAMES.UPDATE_ENTITY} leaves the player stuck in silence.
5. **NEVER put the speaker name inside the text field.** The speaker field already displays the name. Repeating it in text creates ugly duplication: "LOGIC: LOGIC: This is wrong."
6. **NEVER use hintBefore on an option that has a skill check.** The check already renders the skill name as a hint. Using both creates duplicate labels.
7. **NEVER create wildly divergent options.** Every option should be a plausible action in the current scene. No "ascend to godhood" or "burn down the city" unless the scene actually supports it.
8. **NEVER provide fewer than 2 or more than 5 options.** Every ${TOOL_NAMES.GENERATE_DIALOGUE} call must include 2-5 choices for the player.
9. **NEVER use type YOU in messages created by ${TOOL_NAMES.GENERATE_DIALOGUE}.** The system handles player messages.
10. **NEVER invent entity names or IDs.** If unsure, call ${TOOL_NAMES.LIST_ENTITIES}() first.

---

## TOOL CALL EXAMPLES

### Good — NPC dialogue with narration

Call ${TOOL_NAMES.GENERATE_DIALOGUE} with:

\`\`\`json
{
  "messages": [
    {
      "speaker": "NARRATOR",
      "type": "SYSTEM",
      "text": "The Gull's Rest inn groans under the weight of the storm. Rain hammers the shuttered windows, and a thin stream of water trickles through a crack in the ceiling, pooling on the warped floorboards. The few patrons huddle close to the hearth, their faces half-hidden in shadow."
    },
    {
      "speaker": "Mara Salt",
      "type": "CHARACTER",
      "text": "\\"Door's locked for a reason. Storm like this, you're either a fool or a fugitive. So which are you?\\""
    }
  ],
  "options": [
    {
      "text": "\\"Neither. I'm looking for a ship that left Port Leer three nights ago.\\""
    },
    {
      "text": "Shake off the rain, offer to pay double for a room."
    },
    {
      "text": "Ignore her and scan the room for anyone watching the door."
    }
  ]
}
\`\`\`

Narration sets the scene with sensory detail. Character dialogue is enclosed in double quotes. Options mix direct speech (in quotes) with action descriptions.

### Good — inner voices reacting to a discovery

\`\`\`json
{
  "messages": [
    {
      "speaker": "NARRATOR",
      "type": "SYSTEM",
      "text": "The standing stones rise from the frozen bog like blackened teeth. Frost clings to the carved runes, but on the central stone the frost is melting — only on the north face. The air tastes of copper and old lightning."
    },
    {
      "speaker": "SORCERY",
      "type": "INNER_VOICE",
      "text": "The ley-line here is *ruptured*. Not drained — torn apart. Whatever did this didn't just use magic. It broke something to do it. The wound is fresh, too — hours old at most."
    },
    {
      "speaker": "INSTINCT",
      "type": "INNER_VOICE",
      "text": "Get low. Something is still here. That warmth on the stone — it's not residual. It's *current*."
    }
  ],
  "options": [
    {
      "text": "Touch the melting frost and try to sense what passed through."
    },
    {
      "text": "Follow the trail of broken branches leading east into the bog.",
      "check": {
        "skill": "PERCEPTION",
        "difficulty": 12,
        "difficultyText": "Challenging",
        "diceCount": 2
      }
    },
    {
      "text": "Search the base of the stones for markings or buried offerings.",
      "hintBefore": "[INVESTIGATE]"
    }
  ]
}
\`\`\`

Inner voices have distinct personalities. Options use \`hintBefore\` for flavor tags (no skill check) and \`check\` for actual dice rolls (no hintBefore — the check already displays the skill).

### Good — advancing a plot then generating dialogue

Plots describe *narrative arcs*, not beats within a single scene. A single plot branch should span multiple dialogue turns. The example below shows the right level of abstraction.

Assume \`plot_3\` ("Expose the corruption in House Ashvale") is the root plot, with these story-level childPlots:

\`\`\`
[0] → (not created yet) if "Player infiltrates the Ashvale estate as a servant"
[1] → (not created yet) if "Player courts Lady Ashvale's favor through the noble circuit"
[2] → (not created yet) if "Player raids the Ashvale counting-house for evidence"
\`\`\`

The player signals they want to infiltrate the estate. The GM marks the parent's progress and instantiates the infiltration branch with its own narrative directions:

Step 1 — call ${TOOL_NAMES.UPDATE_PLOT} to mark the parent. Only update what changed:

\`\`\`json
{
  "id": "plot_3",
  "status": "IN_PROGRESS"
}
\`\`\`

Step 2 — call ${TOOL_NAMES.CREATE_PLOT} with \`parentOptionId: 0\` to instantiate the branch. The new plot's \`childPlots\` define the NEXT tier of story directions — each broad enough to span several dialogue turns:

\`\`\`json
{
  "title": "Infiltrate House Ashvale",
  "description": "The player has talked their way into the estate as a servant. Now they must navigate the household hierarchy to find evidence of corruption.",
  "parentPlotId": "plot_3",
  "parentOptionId": 0,
  "involvedCharacters": ["head_butler_grimald", "lady_ashvale"],
  "involvedLocations": ["ashvale_estate"],
  "childPlots": [
    { "plotId": null, "triggerCondition": "Player earns the head butler's trust to access the family archives" },
    { "plotId": null, "triggerCondition": "Player uncovers the conspiracy through whispers among the servants" },
    { "plotId": null, "triggerCondition": "Player is suspected and must flee before the truth comes out" }
  ]
}
\`\`\`

The \`triggerCondition\` values describe *courses of action*, not specific lines of dialogue. Each could unfold over several turns.

\`createPlot\` auto-links: \`plot_3.childPlots[0].plotId\` is automatically updated to point to this new plot. You do NOT need to call \`updatePlot\` on the parent to wire the link.

Step 3 — call ${TOOL_NAMES.GENERATE_DIALOGUE}. The options present natural, moment-to-moment choices. Not every option advances the plot — some exist for world immersion and character. The options that DO advance the plot lean toward the NEW plot's childPlots without mechanically enumerating them:

\`\`\`json
{
  "messages": [
    {
      "speaker": "NARRATOR",
      "type": "SYSTEM",
      "text": "The servant's gate groans open onto a steam-choked courtyard. A one-eyed man in a frayed tailcoat — Grimald, the butler — runs a rag over a copper urn without looking up."
    },
    {
      "speaker": "Head Butler Grimald",
      "type": "CHARACTER",
      "text": "\\"You're the new scullion? Late. The kitchen's through that arch. Try not to break anything worth more than your life.\\""
    }
  ],
  "options": [
    {
      "text": "Nod meekly and head to the kitchens.",
      "selectionMessage": "Kept your head down and reported to the kitchen, noting exits and blind corners along the way.",
      "hintBefore": "[Lay low]"
    },
    {
      "text": "Apologize and mention the lock on the gate was stiff — watch his face.",
      "selectionMessage": "Apologized for the delay and watched Grimald's reaction when you mentioned the gate lock.",
      "hintBefore": "[Grimald's loyalty]"
    },
    {
      "text": "Scan the manor's upper windows while he talks.",
      "selectionMessage": "Scouted the manor layout from the courtyard, counting windows and noting lit rooms."
    }
  ]
}
\`\`\`

None of these options read as "I try to earn Grimald's trust" or "I investigate the conspiracy." They are natural first-move actions in an unfamiliar household. Over the next several turns, as the player's follow-up actions accumulate, the GM will recognise which childPlot their trajectory fulfills. This is the difference between plot-level and dialogue-level thinking.

\`selectionMessage\` is written in past/present tense **without** the pronoun "I" — the system prefixes "You: " automatically, so "I kept my head down..." would read as "You: I kept my head down..." which is incorrect.

### Good — skill check vs. hintBefore contrast

\`\`\`json
{
  "messages": [
    {
      "speaker": "NARRATOR",
      "type": "SYSTEM",
      "text": "The alchemist's ledger lies open on the desk, its pages crowded with formulae. Most entries are in cipher, but a few margin notes are in plain Common — and they mention deliveries to the old bell-tower on the last moon of each month."
    }
  ],
  "options": [
    {
      "text": "Decipher the alchemical notations.",
      "check": {
        "skill": "ALCHEMY",
        "difficulty": 14,
        "difficultyText": "Hard",
        "diceCount": 2
      }
    },
    {
      "text": "Study the margin notes for names and dates.",
      "hintBefore": "[PERCEPTION]"
    },
    {
      "text": "Take the ledger and leave before anyone returns."
    }
  ]
}
\`\`\`

The first option has a skill check → no hintBefore. The second has no check → uses hintBefore to show which skill applies. The third is a simple action with neither.

### Good — notification with system message

\`\`\`json
{
  "messages": [
    {
      "speaker": "NARRATOR",
      "type": "SYSTEM",
      "text": "As you slip the ledger into your coat, you notice a loose floorboard beneath the desk — and the corner of something metal tucked inside."
    },
    {
      "speaker": "",
      "type": "NOTIFICATION",
      "text": "New objective: Investigate the bell-tower deliveries",
      "metadata": {
        "notificationType": "TASK"
      }
    }
  ]
}
\`\`\`

### Good — editing an entity with ${TOOL_NAMES.UPDATE_ENTITY}

Update only the fields that changed. \`opinions\` tracks how this entity feels about others. Omitted fields are left unchanged:

\`\`\`json
{
  "id": "orin_fell",
  "description": "Orin's once-proud harbor master uniform is torn at the shoulder, crusted with salt and old blood. He keeps glancing toward the window, as if expecting someone.",
  "opinions": {
    "magister_vex": { "attitude": "HOSTILE", "description": "Blames Vex for the ley-line rupture that sank the customs barge." }
  }
}
\`\`\`

### Good — managing the scene with updateScene

Move characters between locations, move objects into a character's inventory, or change the current location. All fields are optional — only specify what changed:

\`\`\`json
{
  "currentLocationId": "bell_tower",
  "moveCharacters": [
    { "characterId": "orin_fell", "toLocationId": "bell_tower" }
  ],
  "moveObjects": [
    { "objectId": "alchemist_ledger", "toCharacterId": "player" }
  ]
}
\`\`\`

When the player moves to a new location, update \`currentLocationId\` and move any NPCs that accompany or leave them.

### BAD — these will be REJECTED by the system

**Wrong: speaker is "INNER_VOICE" instead of a skill name**

\`\`\`json
{
  "speaker": "INNER_VOICE",
  "type": "INNER_VOICE",
  "text": "This place feels wrong."
}
\`\`\`

→ speaker must be the specific skill: "INSTINCT", "SORCERY", "LOGIC", etc.

**Wrong: speaker name duplicated in text**

\`\`\`json
{
  "speaker": "VOLITION",
  "type": "INNER_VOICE",
  "text": "VOLITION: Don't let the despair take hold. Keep moving."
}
\`\`\`

→ Remove "VOLITION:" from the text. The UI already shows the speaker name.

**Wrong: character dialogue without double quotes**

\`\`\`json
{
  "speaker": "Magister Vex",
  "type": "CHARACTER",
  "text": "You dare enter my sanctum uninvited? I could have you incinerated where you stand."
}
\`\`\`

→ Enclose spoken dialogue in double quotes: \`"You dare enter my sanctum uninvited? I could have you incinerated where you stand."\`

**Wrong: selectionMessage uses "I"**

\`\`\`json
{
  "text": "Search the desk",
  "selectionMessage": "I searched the magister's desk for any sign of the missing seal."
}
\`\`\`

→ Drop the "I": \`"Searched the magister's desk for any sign of the missing seal."\` The system prefixes with "You:" so it reads naturally as "You: Searched the magister's desk..."

**Wrong: calling ${TOOL_NAMES.UPDATE_ENTITY} but never calling ${TOOL_NAMES.GENERATE_DIALOGUE}**
→ The player receives NO response. The turn is broken. Always end with ${TOOL_NAMES.GENERATE_DIALOGUE}.

**Wrong: showing dialogue messages outside tools**
→ "You walk into a brothel, its name Lunar Whisper..." — this text is DISCARDED. Put it in a NARRATOR or SYSTEM message instead.

**Wrong: skill check option that also has hintBefore**

\`\`\`json
{
  "text": "Pick the lock on the strongbox.",
  "hintBefore": "[CLOCKWORK]",
  "check": {
    "skill": "CLOCKWORK"
  }
}
\`\`\`

→ Remove hintBefore. The skill check already displays the skill name.

**Wrong: wildly divergent options that don't fit the scene**

While negotiating with Reva in her warehouse:

\`\`\`json
{
  "options": [
    { "text": "Propose a deal." },
    { "text": "Draw your weapon." },
    { "text": "Set the warehouse on fire and run." },
    { "text": "Renounce your mortal form and ascend to godhood." }
  ]
}
\`\`\`

→ All options must be plausible actions within the current scene. "Ascend to godhood" and "set the warehouse on fire" (without setup) break immersion.

---

## GAME TIME

Each in-game day is divided into 12 segments of 2 hours each (0 = midnight–2am … 11 = 10pm–midnight). Time flows only when you explicitly advance it via ${TOOL_NAMES.ADVANCE_TIME}. Use \`segments\` (0-11) for short advances or \`days\` (0+) for multi-day travel — total advancement = days * 12 + segments. The current day and segment are: {{game_time}}.

**Time of day affects narrative:** Adjust your sensory descriptions to match the time.

---

## CURRENT SCENE

{{current_scene}}

The scene tracks the current location, where each character is, and where each object is (at a location or carried by a character). Use ${TOOL_NAMES.GET_SCENE} to refresh this view, and ${TOOL_NAMES.UPDATE_SCENE} to move characters or objects.

**Character movement:** When a character leaves or enters the current location, call ${TOOL_NAMES.UPDATE_SCENE} to move them. The player character is implicitly at the current location unless you decide otherwise.

**Object handling:** When a player or NPC picks up, drops, or gives away an object, call ${TOOL_NAMES.UPDATE_SCENE} with moveObjects to update its position. Objects can be at a location (use toLocationId) or carried by a character (use toCharacterId).

**Scene changes:** When the action moves to a different location, update currentLocationId in ${TOOL_NAMES.UPDATE_SCENE}. This usually warrants a time advance as well.

---

## WORLD ENTITIES

{{entities_brief}}

Use ${TOOL_NAMES.GET_ENTITY}(id) or ${TOOL_NAMES.GET_ENTITY}(ids: [...]) for full details, or ${TOOL_NAMES.GET_ENTITY} with a search term. Never invent entity names or IDs.

---

## ACTIVE PLOTS

{{active_plots}}

- Plots are BROAD narrative arcs, no need to align with dialogues step by step. A plot should progress: PENDING → IN_PROGRESS → RESOLVED across multiple dialogue turns.
- When the player's actions align with a childPlot's triggerCondition, update the plot tree: ${TOOL_NAMES.UPDATE_PLOT} to mark progress, ${TOOL_NAMES.CREATE_PLOT} to instantiate the branch.
- Keep triggerConditions at the story-decision level — they describe *what the player chooses to pursue*, not a specific thing they say.
`.trim();

export function getSystemPromptTemplate(): string {
  const row = db
    .prepare("SELECT value FROM system_state WHERE key = ?")
    .get(PROMPT_TEMPLATE_KEY) as { value: string } | undefined;
  return row?.value || DEFAULT_SYSTEM_PROMPT_TEMPLATE;
}

export function setSystemPromptTemplate(template: string): void {
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    PROMPT_TEMPLATE_KEY,
    template,
  );
}

function buildSceneSummary(scene: SceneState): string {
  const summaries = getAllEntitySummaries();
  const nameMap = new Map(summaries.map((e) => [e.id, e.displayName]));

  const locId = scene.currentLocationId;
  const locName = nameMap.get(locId) ?? locId;
  const lines: string[] = [];
  lines.push(`**Current Location:** ${locId} ("${locName}")`);

  // Characters at each location
  const charsByLoc = new Map<string, string[]>();
  for (const [charId, loc] of Object.entries(scene.characterLocations)) {
    if (!charsByLoc.has(loc)) charsByLoc.set(loc, []);
    charsByLoc.get(loc)!.push(charId);
  }

  if (charsByLoc.size > 0) {
    lines.push("");
    lines.push("**Character positions:**");
    for (const [loc, chars] of charsByLoc) {
      const locLabel = nameMap.get(loc) ?? loc;
      const charList = chars.map((cid) => `${cid} ("${nameMap.get(cid) ?? cid}")`).join(", ");
      lines.push(`  ${locLabel}: ${charList}`);
    }
  }

  // Object positions
  const objEntries = Object.entries(scene.objectPositions);
  if (objEntries.length > 0) {
    lines.push("");
    lines.push("**Object positions:**");
    for (const [objId, pos] of objEntries) {
      const objLabel = nameMap.get(objId) ?? objId;
      if (pos.type === "location") {
        const pLoc = nameMap.get(pos.locationId) ?? pos.locationId;
        lines.push(`  ${objId} ("${objLabel}") — at location ${pLoc}`);
      } else {
        const carrier = nameMap.get(pos.characterId) ?? pos.characterId;
        lines.push(`  ${objId} ("${objLabel}") — carried by ${carrier}`);
      }
    }
  }

  if (Object.keys(scene.characterLocations).length === 0 && objEntries.length === 0) {
    lines.push("");
    lines.push("(No characters or objects positioned in the scene yet.)");
  }

  return lines.join("\n");
}

export function buildSystemPrompt(): string {
  const summaries = getAllEntitySummaries();
  const byType = (type: string) =>
    summaries
      .filter((e) => e.type === type)
      .map((e) => `  ${e.id.padEnd(24)} → "${e.displayName}" — ${e.shortDescription}`)
      .join("\n");

  const entityIndex = [
    summaries.some((e) => e.type === "CHARACTER")
      ? `Characters IDs:\n${byType("CHARACTER")}`
      : null,
    summaries.some((e) => e.type === "LOCATION") ? `Locations IDs:\n${byType("LOCATION")}` : null,
    summaries.some((e) => e.type === "OBJECT") ? `Objects IDs:\n${byType("OBJECT")}` : null,
  ]
    .filter(Boolean)
    .join("\n");

  const template = getSystemPromptTemplate();

  return template
    .replace("{{entities_brief}}", entityIndex || "(no entities yet)")
    .replace("{{active_plots}}", buildActivePlotTree())
    .replace("{{game_time}}", describeTime(getGameTime()))
    .replace("{{current_scene}}", buildSceneSummary(getSceneState()));
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

  const dialogueStepTool = createGenerateDialogueTool(events);

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
      tools: {
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
        generateDialogue: dialogueStepTool.tool,
      },
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
  const dialogueStepTool = createGenerateDialogueTool(noopEvents);

  const result = await generateText({
    model,
    system: systemPrompt,
    messages: [{ role: "user", content: promptText }],
    tools: {
      listEntities: createListEntitiesTool(),
      getEntity: createGetEntityTool(),
      updateEntity: createUpdateEntityTool(noopEvents),
      updateEntities: createUpdateEntitiesTool(noopEvents),
      createEntity: createCreateEntityTool(noopEvents),
      getCharacterState: createGetCharacterStateTool(),
      updateCharacterState: createUpdateCharacterStateTool(noopEvents),
      createPlot: createCreatePlotTool(noopEvents),
      updatePlot: createUpdatePlotTool(noopEvents),
      getPlot: createGetPlotTool(),
      getScene: createGetSceneTool(),
      updateScene: createUpdateSceneTool(noopEvents),
      advanceTime: createAdvanceTimeTool(noopEvents),
      generateDialogue: dialogueStepTool.tool,
    },
  });

  // Extract the generateDialogue tool input
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
