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

import { tool } from "ai";
import { z } from "zod";
import {
  updateEntity,
  getEntityById,
  getEntitiesByIds,
  getAllEntitySummaries,
  getEntitiesByText,
} from "@/server/models/world";
import { addPlot, updatePlot, getPlotById, getPlotsByIds, getAllPlots } from "@/server/models/plot";
import { nextId } from "@/server/models/ids";
import {
  addFact,
  getFactById,
  getFacts,
  getFactsByIds,
  updateFact,
  removeFact,
} from "@/server/models/facts";
import {
  getSceneState,
  setSceneState,
  getGameTime,
  advanceGameTime,
  describeTime,
} from "@/server/models/scene";
import { PLOT_STATUSES, PlotOption } from "@/types/plot";
import type { TurnEventEmitter } from "@/server/llm/events";
import { DialogueOption, NOTIFICATION_TYPES, SPEAKER_TYPES } from "@/types/dialogue";
import { ENTITY_TYPES, EntityType, SceneState, type Fact } from "@/types/entities";
import { TOOL_NAMES } from "@/shared/constants.ts";

// ── Text verification ──

const disallowedRe =
  /[\p{Emoji}\p{Script=Han}\p{Script=Arabic}\p{Script=Cyrillic}\p{Script=Hiragana}\p{Script=Katakana}]/u;

function isAllowedText(str: string): boolean {
  if (!disallowedRe.test(str)) return true;
  // \p{Emoji} matches ASCII digits 0-9, #, * (emoji keycap bases) — allow them
  return /^[0-9#*]$/.test(str);
}

function checkText(value: unknown, context: string): string | null {
  const str = typeof value === "string" ? value : JSON.stringify(value);
  const disallowed = [...str].filter((c) => !isAllowedText(c));
  if (disallowed.length !== 0) {
    const unique = [...new Set(disallowed)].slice(0, 10);
    return `TEXT VERIFICATION FAILED in ${context}: disallowed characters detected [${unique.join(",")}]. Only Latin-script text and typographic punctuation (no emoji, no non-Latin scripts) is allowed. Please retry with allowed content.`;
  }
  return null;
}

// ── Error-handling wrapper ──

function wrapSafe<T>(
  fn: (args: T) => Promise<string>,
  toolName: string,
): (args: T) => Promise<string> {
  return async (args: T) => {
    const inputError = checkText(args, `${toolName} input`);
    if (inputError) return inputError;

    try {
      const result = await fn(args);
      const outputError = checkText(result, `${toolName} output`);
      if (outputError) return outputError;
      return result;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(`[${toolName}] execute error:`, err);
      return `ERROR: Tool "${toolName}" failed unexpectedly: ${msg}. Please retry or use a different approach.`;
    }
  };
}

// ── Helpers ──

export function mapToDialogueOption(
  o: Record<string, unknown>,
  i: number,
  baseId: string,
): DialogueOption {
  const optId = (o.id as string) || `opt_${baseId}_${i}`;
  const check = o.check as Record<string, unknown> | undefined;
  return {
    id: optId,
    text: (o.text as string) || "",
    selectionMessage: o.selectionMessage as string | undefined,
    hintBefore: o.hintBefore as string | undefined,
    hintAfter: o.hintAfter as string | undefined,
    check: check
      ? {
          skill: check.skill as string,
          difficulty: check.difficulty as number,
          difficultyText: (check.difficultyText as string) || "",
          diceCount: (check.diceCount as number) ?? 2,
          isRed: check.isRed as boolean | undefined,
          conditions: ((check.conditions as unknown[]) || []).map((c: unknown, ci: number) => {
            const cond = c as Record<string, unknown>;
            return {
              expression: cond.expression as string,
              label: cond.label as string | undefined,
              color: cond.color as string | undefined,
              stepId: (cond.stepId as string) || `step_${optId}_res_${ci}`,
            };
          }),
        }
      : undefined,
  };
}

// ── Tool factories ──

const entityGetSchema = z.object({
  type: z.enum(ENTITY_TYPES).optional().describe("Optional filter by entity type."),
});

export function createListEntitiesTool() {
  return tool({
    title: "List Entities",
    description: "Returns the id, displayName, type, and shortDescription of all world entities.",
    inputSchema: entityGetSchema,
    execute: wrapSafe(async (args: z.infer<typeof entityGetSchema>) => {
      const summaries = getAllEntitySummaries(args.type);
      if (summaries.length === 0) return "No entities found.";
      return JSON.stringify(summaries, null, 2);
    }, TOOL_NAMES.LIST_ENTITIES),
  });
}

const getEntitySchema = z.object({
  id: z.string().optional().describe("Exact entity ID for single lookup (e.g. 'madam_vespera')."),
  ids: z
    .array(z.string())
    .optional()
    .describe(
      "Array of entity IDs for bulk lookup. Returns results in the same order, skipping missing IDs.",
    ),
  search: z
    .string()
    .optional()
    .describe("Text to search for in entity names/descriptions (up to 5 results)."),
});

export function createGetEntityTool() {
  return tool({
    title: "Get Entity",
    description:
      "Get full details of world entities. Provide an id for single lookup, ids array for bulk lookup, or a search term for text search (case-insensitive match on name/description, up to 5 results).",
    inputSchema: getEntitySchema,
    execute: wrapSafe(async (args: z.infer<typeof getEntitySchema>): Promise<string> => {
      if (args.id && args.ids && args.search) {
        return "ERROR: You can only search in only one of these ways: provide 'id' for single lookup, 'ids' for bulk lookup, or 'search' for text search.";
      }
      if (!args.id && !args.ids && !args.search) {
        return "ERROR: Search intention is unknown. Provide 'id' for single lookup, 'ids' for bulk lookup, or 'search' for text search.";
      }
      if (args.id) {
        const entity = getEntityById(args.id);
        if (!entity) {
          return `ERROR: Entity '${args.id}' not found. You may call ${TOOL_NAMES.LIST_ENTITIES}() to discover valid IDs.`;
        }
        return JSON.stringify(entity, null, 2);
      }
      if (args.ids && args.ids.length > 0) {
        const results = getEntitiesByIds(args.ids);
        if (results.length === 0) {
          return `ERROR: None of the requested IDs were found: [${args.ids.join(", ")}]. You may call ${TOOL_NAMES.LIST_ENTITIES}() to discover valid IDs.`;
        }
        if (results.length < args.ids.length) {
          const foundIds = new Set(results.map((e) => e.id));
          const missing = args.ids.filter((id) => !foundIds.has(id));
          return JSON.stringify(
            {
              note: `The following IDs were not found: [${missing.join(", ")}]. They may have been removed or misspelled.`,
              results,
            },
            null,
            2,
          );
        }
        return JSON.stringify(
          {
            note: "All IDs is successfully queried",
            results,
          },
          null,
          2,
        );
      }
      const results = getEntitiesByText(args.search!);
      if (results.length === 0) {
        return `No entities matched '${args.search}'. You may call ${TOOL_NAMES.LIST_ENTITIES}() to see all entities.`;
      }
      return JSON.stringify(results, null, 2);
    }, TOOL_NAMES.GET_ENTITY),
  });
}

const entitySchema = z.object({
  id: z.string().describe("The unique ID of the entity to update (e.g. 'madam_vespera')."),
  shortDescription: z.string().optional().describe("New concise label."),
  longDescription: z.string().optional().describe("New detailed observation."),
  attributes: z
    .record(z.string(), z.string())
    .optional()
    .describe("Physical or mental traits (merged)."),
  opinions: z
    .record(z.string(), z.string())
    .optional()
    .describe("How this character feels about others (merged). Only valid for CHARACTER entities."),
});

export function createUpdateEntityTool(events: TurnEventEmitter) {
  return tool({
    title: "Update Entity",
    description:
      "Mutate a single world entity's description, attributes, or opinions. One entity per call. Reports an error if the entity ID does not exist.",
    inputSchema: entitySchema,
    execute: wrapSafe(async (args: z.infer<typeof entitySchema>) => {
      const existing = getEntityById(args.id);
      if (!existing) {
        return `ERROR: Entity '${args.id}' not found. You may use ${TOOL_NAMES.LIST_ENTITIES}() to discover valid IDs.`;
      }
      updateEntity(args);
      const changes: Record<string, unknown> = {};
      if (args.longDescription != null) changes.longDescription = args.longDescription;
      if (args.shortDescription != null) changes.shortDescription = args.shortDescription;
      if (args.attributes) changes.attributes = args.attributes;
      if (args.opinions) changes.opinions = args.opinions;
      events.emitWorldUpdate(args.id, changes);
      return `Entity with name '${existing.displayName}' (id: ${args.id}) updated.`;
    }, TOOL_NAMES.UPDATE_ENTITY),
  });
}

const plotOptionSchema = z.object({
  plotId: z.string().nullable().describe("ID of the child plot, or null if not created yet."),
  triggerCondition: z.string().describe("What player action activates this branch."),
});

const plotSchema = z.object({
  title: z.string().describe("Concise title of the plot/quest."),
  description: z.string().describe("Detailed description of what this plot is about."),
  status: z.enum(PLOT_STATUSES).optional().describe("Initial status (default: PENDING)."),
  involvedLocations: z
    .array(z.string())
    .optional()
    .describe("Entity IDs of involved locations (prefer one)."),
  involvedCharacters: z
    .array(z.string())
    .optional()
    .describe("Entity IDs of involved characters (player is implicit)."),
  parentPlotId: z
    .string()
    .nullable()
    .optional()
    .describe("ID of the parent plot. Omit or set null for a root plot."),
  parentOptionId: z
    .number()
    .nullable()
    .optional()
    .describe("Index into parent.childPlots that this plot fulfils."),
  childPlots: z
    .array(plotOptionSchema)
    .optional()
    .describe("Pre-defined branch options for this plot."),
});

export function createCreatePlotTool(events: TurnEventEmitter) {
  return tool({
    title: "Create Plot",
    description:
      "Create a new plot node in the story tree. If this is the first plot, omit parentPlotId to create the root. Otherwise provide parentPlotId and parentOptionId (index into parent's childPlots array) to link it into the tree. The parent's childPlots[parentOptionId].plotId will be auto-updated.",
    inputSchema: plotSchema,
    execute: wrapSafe(async (args: z.infer<typeof plotSchema>) => {
      const plotId = `plot_${nextId()}`;
      const result = addPlot({
        id: plotId,
        title: args.title,
        description: args.description,
        status: args.status ?? "PENDING",
        involvedLocations: args.involvedLocations ?? [],
        involvedCharacters: args.involvedCharacters ?? [],
        parentPlotId: args.parentPlotId ?? null,
        parentOptionId: args.parentOptionId ?? null,
        childPlots: (args.childPlots ?? []) as PlotOption[],
      });

      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }

      events.emitPlotCreate(plotId, args.title, args.parentPlotId ?? null);
      return `Plot created: "${args.title}" (${plotId}).`;
    }, TOOL_NAMES.CREATE_PLOT),
  });
}

const plotEditSchema = z.object({
  id: z.string().describe("The ID of the plot to update."),
  status: z.enum(PLOT_STATUSES).optional().describe("New status for the plot."),
  description: z.string().optional().describe("Updated plot description."),
  involvedLocations: z
    .array(z.string())
    .optional()
    .describe("Replacement list of involved location entity IDs."),
  involvedCharacters: z
    .array(z.string())
    .optional()
    .describe("Replacement list of involved character entity IDs."),
  childPlots: z
    .array(plotOptionSchema)
    .optional()
    .describe("Replacement list of branch options (replaces all existing childPlots)."),
});

export function createUpdatePlotTool(events: TurnEventEmitter) {
  return tool({
    title: "Update Plot",
    description:
      "Update an existing plot's status, description, involved entities, or childPlots options. Only PENDING or IN_PROGRESS plots can be edited — RESOLVED plots are locked. Reports an error if the plot ID does not exist, the plot is RESOLVED, or the change would break the plot tree.",
    inputSchema: plotEditSchema,
    execute: wrapSafe(async (args: z.infer<typeof plotEditSchema>) => {
      const result = updatePlot(args.id, {
        status: args.status,
        description: args.description,
        involvedLocations: args.involvedLocations,
        involvedCharacters: args.involvedCharacters,
        childPlots: args.childPlots as PlotOption[] | undefined,
      });

      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }

      const changes: Record<string, unknown> = {};
      if (args.status !== undefined) changes.status = args.status;
      if (args.description !== undefined) changes.description = args.description;
      if (args.involvedLocations !== undefined) changes.involvedLocations = args.involvedLocations;
      if (args.involvedCharacters !== undefined)
        changes.involvedCharacters = args.involvedCharacters;
      if (args.childPlots !== undefined) changes.childPlots = args.childPlots;
      events.emitPlotEdit(args.id, changes);

      const plot = getPlotById(args.id);
      if (!plot) return `Plot ${args.id} updated but could not be re-read.`;
      return `Plot "${plot.title}" (${args.id}) updated.`;
    }, TOOL_NAMES.UPDATE_PLOT),
  });
}

const getPlotSchema = z.object({
  id: z.string().optional().describe("Exact plot ID to fetch."),
  ids: z.array(z.string()).optional().describe("Array of plot IDs to bulk fetch."),
  status: z.enum(PLOT_STATUSES).optional().describe("Filter by status. Omit to return all plots."),
});

export function createGetPlotTool() {
  return tool({
    title: "Get Plot",
    description:
      "Retrieve plot(s): by single ID, by multiple IDs (bulk), or filter by status. Returns full plot data including childPlots.",
    inputSchema: getPlotSchema,
    execute: wrapSafe(async (args: z.infer<typeof getPlotSchema>) => {
      if (args.id && args.ids && args.ids.length !== 0) {
        return "ERROR: Provide either 'id' for a single plot or 'ids' for bulk fetch, not both.";
      }
      if (args.id) {
        const plot = getPlotById(args.id);
        if (!plot) {
          return `ERROR: Plot '${args.id}' not found. You may use ${TOOL_NAMES.GET_PLOT}() without an id to list all plots.`;
        }
        return JSON.stringify(plot, null, 2);
      }
      if (args.ids && args.ids.length > 0) {
        const plots = getPlotsByIds(args.ids);
        if (plots.length === 0) {
          return `No plots found with the provided IDs: [${args.ids.join(", ")}].`;
        }
        const found = new Set(plots.map((p) => p.id));
        const missing = args.ids.filter((id) => !found.has(id));
        const result: Record<string, unknown> = { plots };
        if (missing.length > 0) {
          result.missingIds = missing;
        }
        return JSON.stringify(result, null, 2);
      }
      const all = getAllPlots();
      const filtered = !args.status ? all : all.filter((p) => p.status === args.status);
      if (filtered.length === 0)
        return `No plots found${args.status ? ` with status ${args.status}` : ""}.`;
      return JSON.stringify(filtered, null, 2);
    }, TOOL_NAMES.GET_PLOT),
  });
}

const messageSchema = z.object({
  speaker: z
    .string()
    .describe(
      "Name of the speaker (no '_' between words, e.g. 'LOGIC', 'Orin Fell', 'NARRATOR', 'INSTINCT', 'SORCERY')",
    ),
  type: z.enum(SPEAKER_TYPES),
  text: z.string().describe("The dialogue text, supports markdown."),
  metadata: z
    .object({
      notificationType: z.enum(NOTIFICATION_TYPES).optional(),
    })
    .optional(),
});

const checkConditionSchema = z.object({
  expression: z.string().describe("JS expression e.g. 'success' or 'total < difficulty'"),
  label: z.string().optional(),
  color: z.string().optional(),
});

const skillCheckSchema = z.object({
  skill: z.string().describe("The skill to check (e.g. 'LOGIC')"),
  difficulty: z.number().describe("Numerical difficulty (e.g. 10)"),
  difficultyText: z.string().describe("Textual difficulty (e.g. 'Challenging')"),
  diceCount: z.number().default(2),
  isRed: z.boolean().optional().describe("High-stakes, one-time check."),
  conditions: z.array(checkConditionSchema).describe("Outcome conditions."),
});

const optionSchema = z.object({
  id: z.string().optional(),
  text: z.string().describe("Short imperative button label (e.g. 'Try to convince the guard')."),
  selectionMessage: z
    .string()
    .optional()
    .describe(
      "Optional sentence for the YOU message in dialogue history after the player selects this option. Write in past or present tense WITHOUT the pronoun 'I' — the system prefixes with 'You:' automatically (e.g. 'Tried to convince the guard to let us pass.' reads as 'You: Tried to convince...'). Using 'I' would produce the awkward 'You: I tried...'. If omitted, the text field is used with any [SKILL] prefix removed.",
    ),
  hintBefore: z
    .string()
    .optional()
    .describe("Hint shown before the text, e.g. [Logic]. Do not overuse it."),
  hintAfter: z
    .string()
    .optional()
    .describe("Hint shown after the text, e.g. [Red Check]. Do not overuse it."),
  check: skillCheckSchema.optional(),
});

const dialogueStepSchema = z.object({
  messages: z.array(messageSchema).describe("The sequence of messages in this dialogue step."),
  options: z.array(optionSchema).describe("The choices presented to the player."),
});

export function createGenerateDialogueTool(_events: TurnEventEmitter) {
  let lastCallValid = false;

  const dialogueTool = tool({
    description:
      "Generate the narrative dialogue steps and final player choices. This is the ONLY way to communicate to the player. Options should align with the active plot's childPlots.",
    inputSchema: dialogueStepSchema,
    execute: async (args: z.infer<typeof dialogueStepSchema>) => {
      const errors: string[] = [];

      for (const msg of args.messages) {
        if (msg.speaker === "INNER_VOICE") {
          errors.push(
            `A message uses speaker="INNER_VOICE" — INNER_VOICE is a type, not a speaker name. Use the specific skill name as the speaker (e.g. "LOGIC", "INSTINCT", "SORCERY").`,
          );
          break;
        }
      }

      // text verification on messages
      for (let i = 0; i < args.messages.length; i++) {
        const msg = args.messages[i];
        const speakerError = checkText(
          msg.speaker,
          `${TOOL_NAMES.GENERATE_DIALOGUE} messages[${i}].speaker`,
        );
        if (speakerError) {
          errors.push(speakerError);
          break;
        }
        const textError = checkText(
          msg.text,
          `${TOOL_NAMES.GENERATE_DIALOGUE} messages[${i}].text`,
        );
        if (textError) {
          errors.push(textError);
          break;
        }
      }

      if (!args.options || args.options.length === 0) {
        errors.push(
          "Missing options — every ${TOOL_NAMES.GENERATE_DIALOGUE} call must include 2-5 choices for the player. Provide options that respond to the current scene.",
        );
      }

      if (args.options) {
        for (let i = 0; i < args.options.length; i++) {
          const opt = args.options[i];
          if (opt.check && opt.hintBefore) {
            errors.push(
              `Option ${i + 1} has both a skill check and hintBefore. The skill check already renders the skill name — omit hintBefore for this option.`,
            );
          }
        }

        // text verification on options
        for (let i = 0; i < args.options.length; i++) {
          const opt = args.options[i];
          const textError = checkText(
            opt.text,
            `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].text`,
          );
          if (textError) {
            errors.push(textError);
            break;
          }
          if (opt.hintBefore) {
            const hintError = checkText(
              opt.hintBefore,
              `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].hintBefore`,
            );
            if (hintError) {
              errors.push(hintError);
              break;
            }
          }
          if (opt.hintAfter) {
            const hintError = checkText(
              opt.hintAfter,
              `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].hintAfter`,
            );
            if (hintError) {
              errors.push(hintError);
              break;
            }
          }
          if (opt.selectionMessage) {
            const selMsgError = checkText(
              opt.selectionMessage,
              `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].selectionMessage`,
            );
            if (selMsgError) {
              errors.push(selMsgError);
              break;
            }
          }
        }
      }

      if (errors.length > 0) {
        lastCallValid = false;
        return `VALIDATION FAILED — call ${TOOL_NAMES.GENERATE_DIALOGUE} again with corrections:\n${errors.map((e) => `• ${e}`).join("\n")}`;
      }

      lastCallValid = true;
      return "Dialogue successfully streamed.";
    },
  });

  return { tool: dialogueTool, wasValid: () => lastCallValid };
}

export function createAdvanceTimeTool(events: TurnEventEmitter) {
  return tool({
    title: "Advance Time",
    description:
      "Advance the in-game clock by N segments (0-11, where each segment is 2 hours). Use 0 to describe the current time without advancing. Use this when the player's action takes time. Describe why time passes in the reason field.",
    inputSchema: z.object({
      segments: z
        .number()
        .int()
        .min(0)
        .max(11)
        .describe("Number of 2-hour segments to advance (0-11)."),
      reason: z
        .string()
        .optional()
        .describe(
          "Brief narrative reason for the time advance (e.g. 'The conversation dragged on').",
        ),
    }),
    execute: wrapSafe(async (args: { segments: number; reason?: string }) => {
      const { oldTime, newTime } = advanceGameTime(args.segments);
      events.emitTimeUpdate(newTime.day, newTime.segment, args.segments);
      const reasonStr = args.reason ? ` Reason: ${args.reason}.` : "";
      if (args.segments === 0) {
        return `Time unchanged. It is still ${describeTime(newTime)}.`;
      }
      return `Time advanced by ${args.segments} segment(s).${reasonStr} It is now ${describeTime(newTime)} (was ${describeTime(oldTime)}).`;
    }, TOOL_NAMES.ADVANCE_TIME),
  });
}

export function createUpdateSceneTool(events: TurnEventEmitter) {
  return tool({
    title: "Update Scene",
    description:
      "Update the current scene: change the active location, move characters between locations, or move objects (to a location or into a character's possession). All fields are optional — only specified changes are applied. The scene tracks who is where and who is carrying what.",
    inputSchema: z.object({
      currentLocationId: z
        .string()
        .optional()
        .describe("Change the current scene's location to this entity ID."),
      moveCharacters: z
        .array(
          z.object({
            characterId: z.string().describe("Character entity ID to move."),
            locationId: z.string().describe("Destination location entity ID."),
          }),
        )
        .optional()
        .describe("Characters to relocate to a different location."),
      moveObjects: z
        .array(
          z.object({
            objectId: z.string().describe("Object entity ID to move."),
            toLocationId: z
              .string()
              .optional()
              .describe("Location entity ID to place the object at."),
            toCharacterId: z
              .string()
              .optional()
              .describe("Character entity ID to give the object to (carried)."),
          }),
        )
        .optional()
        .describe(
          "Objects to move. Provide toLocationId to place at a location, or toCharacterId to give to a character (not both).",
        ),
    }),
    execute: wrapSafe(
      async (args: {
        currentLocationId?: string;
        moveCharacters?: { characterId: string; locationId: string }[];
        moveObjects?: {
          objectId: string;
          toLocationId?: string;
          toCharacterId?: string;
        }[];
      }) => {
        const scene = getSceneState();
        const changes: string[] = [];

        if (args.currentLocationId) {
          scene.currentLocationId = args.currentLocationId;
          changes.push(`Current location set to '${args.currentLocationId}'.`);
        }

        if (args.moveCharacters) {
          for (const { characterId, locationId } of args.moveCharacters) {
            scene.characterLocations[characterId] = locationId;
            changes.push(`Moved character '${characterId}' to location '${locationId}'.`);
          }
        }

        if (args.moveObjects) {
          for (const { objectId, toLocationId, toCharacterId } of args.moveObjects) {
            if (toLocationId && toCharacterId) {
              return `ERROR: Object '${objectId}' has both toLocationId and toCharacterId — choose one.`;
            }
            if (!toLocationId && !toCharacterId) {
              return `ERROR: Object '${objectId}' needs either toLocationId or toCharacterId.`;
            }
            if (toLocationId) {
              scene.objectPositions[objectId] = {
                type: "location",
                locationId: toLocationId,
              };
              changes.push(`Placed object '${objectId}' at location '${toLocationId}'.`);
            } else {
              scene.objectPositions[objectId] = {
                type: "character",
                characterId: toCharacterId!,
              };
              changes.push(`Gave object '${objectId}' to character '${toCharacterId}'.`);
            }
          }
        }

        setSceneState(scene);
        events.emitSceneUpdate(scene);

        if (changes.length === 0) {
          return "Scene unchanged. No fields were specified.";
        }
        return `Scene updated:\n${changes.map((c) => `- ${c}`).join("\n")}`;
      },
      TOOL_NAMES.UPDATE_SCENE,
    ),
  });
}

export function createGetSceneTool() {
  return tool({
    title: "Get Scene",
    description:
      "Returns the current game time and full scene state: where each character is, where each object is (and who is carrying it). Use this to check the current situation before making changes.",
    inputSchema: z.object({}),
    execute: wrapSafe(async () => {
      const time = getGameTime();
      const scene = getSceneState();
      return JSON.stringify(
        {
          gameTime: { day: time.day, segment: time.segment, label: describeTime(time) },
          scene,
        },
        null,
        2,
      );
    }, TOOL_NAMES.GET_SCENE),
  });
}

// ── Facts tools ──

export function createAddFactTool(events: TurnEventEmitter) {
  return tool({
    title: "Add Fact",
    description:
      "Record a GM fact — private working memory that persists between turns. Use this to remember narrative state that isn't a plot: suspicions, countdowns, character relationship changes, environmental details, etc. Facts link to related entities, plots, scene, or time for filtering.",
    inputSchema: z.object({
      key: z.string().describe("Short label for the fact (e.g. 'player_suspects_cressida')."),
      value: z.string().describe("The fact value — what the GM needs to remember."),
      relatedEntityIds: z
        .array(z.string())
        .optional()
        .describe("Entity IDs this fact relates to."),
      relatedPlotIds: z
        .array(z.string())
        .optional()
        .describe("Plot IDs this fact relates to."),
      relatedScene: z
        .boolean()
        .optional()
        .describe("Set true if this fact relates to the current scene state."),
      relatedTime: z
        .boolean()
        .optional()
        .describe("Set true if this fact relates to the current game time."),
    }),
    execute: wrapSafe(async (args) => {
      const fact = addFact({
        key: args.key,
        value: args.value,
        relatedEntityIds: args.relatedEntityIds,
        relatedPlotIds: args.relatedPlotIds,
        relatedScene: args.relatedScene,
        relatedTime: args.relatedTime,
      });
      events.emitFactAdd(fact);
      return `Fact recorded: "${args.key}" (${fact.id}).`;
    }, TOOL_NAMES.ADD_FACT),
  });
}

const getFactSchema = z.object({
  id: z.string().optional().describe("Exact fact ID to fetch."),
  ids: z.array(z.string()).optional().describe("Array of fact IDs for bulk fetch."),
  relatedEntityId: z
    .string()
    .optional()
    .describe("Filter facts linked to this entity ID."),
  relatedPlotId: z
    .string()
    .optional()
    .describe("Filter facts linked to this plot ID."),
  relatedScene: z
    .boolean()
    .optional()
    .describe("Filter facts linked (or not) to scene state."),
  relatedTime: z
    .boolean()
    .optional()
    .describe("Filter facts linked (or not) to game time."),
});

export function createGetFactTool() {
  return tool({
    title: "Get Fact",
    description:
      "Retrieve facts: by single ID, multiple IDs (bulk), or filter by related entity, plot, scene, or time. Only returns valid (non-removed) facts.",
    inputSchema: getFactSchema,
    execute: wrapSafe(async (args) => {
      // Single ID
      if (args.id) {
        const fact = getFactById(args.id);
        if (!fact || !fact.isValid) {
          return `ERROR: Fact '${args.id}' not found.`;
        }
        return JSON.stringify(fact, null, 2);
      }

      // Bulk IDs
      if (args.ids && args.ids.length > 0) {
        const facts = getFactsByIds(args.ids);
        if (facts.length === 0) {
          return `No valid facts found for the provided IDs: [${args.ids.join(", ")}].`;
        }
        const found = new Set(facts.map((f) => f.id));
        const missing = args.ids.filter((id) => !found.has(id));
        const result: Record<string, unknown> = { facts };
        if (missing.length > 0) result.missingIds = missing;
        return JSON.stringify(result, null, 2);
      }

      // Filter
      const facts = getFacts({
        relatedEntityId: args.relatedEntityId,
        relatedPlotId: args.relatedPlotId,
        relatedScene: args.relatedScene,
        relatedTime: args.relatedTime,
      });
      if (facts.length === 0) return "No facts found matching the filter.";
      return JSON.stringify(facts, null, 2);
    }, TOOL_NAMES.GET_FACT),
  });
}

const updateFactSchema = z.object({
  id: z.string().describe("ID of the fact to update."),
  key: z.string().optional().describe("New key label."),
  value: z.string().optional().describe("New value."),
  relatedEntityIds: z
    .array(z.string())
    .optional()
    .describe("Replacement list of related entity IDs."),
  relatedPlotIds: z
    .array(z.string())
    .optional()
    .describe("Replacement list of related plot IDs."),
  relatedScene: z.boolean().optional().describe("Whether this relates to scene state."),
  relatedTime: z.boolean().optional().describe("Whether this relates to game time."),
});

export function createUpdateFactTool(events: TurnEventEmitter) {
  return tool({
    title: "Update Fact",
    description:
      "Update an existing fact's key, value, or related links. Only valid facts can be updated. Reports an error if the fact ID does not exist.",
    inputSchema: updateFactSchema,
    execute: wrapSafe(async (args) => {
      const result = updateFact(args.id, {
        key: args.key,
        value: args.value,
        relatedEntityIds: args.relatedEntityIds,
        relatedPlotIds: args.relatedPlotIds,
        relatedScene: args.relatedScene,
        relatedTime: args.relatedTime,
      });

      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }

      const changes: Record<string, unknown> = {};
      if (args.key !== undefined) changes.key = args.key;
      if (args.value !== undefined) changes.value = args.value;
      if (args.relatedEntityIds !== undefined) changes.relatedEntityIds = args.relatedEntityIds;
      if (args.relatedPlotIds !== undefined) changes.relatedPlotIds = args.relatedPlotIds;
      if (args.relatedScene !== undefined) changes.relatedScene = args.relatedScene;
      if (args.relatedTime !== undefined) changes.relatedTime = args.relatedTime;
      events.emitFactUpdate(args.id, changes);

      return `Fact "${result.fact.key}" (${args.id}) updated.`;
    }, TOOL_NAMES.UPDATE_FACT),
  });
}

export function createRemoveFactTool(events: TurnEventEmitter) {
  return tool({
    title: "Remove Fact",
    description:
      "Soft-delete a fact by ID. The fact is marked invalid but retained in the database. Reports an error if the fact ID does not exist.",
    inputSchema: z.object({
      id: z.string().describe("ID of the fact to remove."),
    }),
    execute: wrapSafe(async (args) => {
      const result = removeFact(args.id);
      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }
      events.emitFactRemove(args.id);
      return `Fact '${args.id}' removed.`;
    }, TOOL_NAMES.REMOVE_FACT),
  });
}
