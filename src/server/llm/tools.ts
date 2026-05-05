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
  getSceneState,
  setSceneState,
  getGameTime,
  advanceGameTime,
  describeTime,
} from "@/server/models/scene";
import { PLOT_STATUSES, PlotOption } from "@/types/plot";
import type { TurnEventEmitter } from "@/server/llm/events";
import { DialogueOption, NOTIFICATION_TYPES, SPEAKER_TYPES } from "@/types/dialogue";
import { ENTITY_TYPES, EntityType, SceneState } from "@/types/entities";
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

export function createGetAllEntitiesNameTool() {
  return tool({
    title: "Get All Entities Name",
    description: "Returns the id, displayName, type, and shortDescription of all world entities.",
    inputSchema: z.object({
      type: z.enum(ENTITY_TYPES).optional().describe("Optional filter by entity type."),
    }),
    execute: wrapSafe(async (args: { type?: EntityType }) => {
      const summaries = getAllEntitySummaries(args.type);
      if (summaries.length === 0) return "No entities found.";
      return JSON.stringify(summaries, null, 2);
    }, TOOL_NAMES.GET_ALL_ENTITIES),
  });
}

const queryEntitySchema = z.object({
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

export function createQueryEntityTool() {
  return tool({
    title: "Query Entity",
    description:
      "Get full details of world entities. Provide an id for single lookup, ids array for bulk lookup, or a search term for text search (case-insensitive match on name/description, up to 5 results).",
    inputSchema: queryEntitySchema,
    execute: wrapSafe(async (args: z.infer<typeof queryEntitySchema>) => {
      if (!args.id && !args.ids && !args.search) {
        return "ERROR: Search intention is unknown. Provide 'id' for single lookup, 'ids' for bulk lookup, or 'search' for text search.";
      }
      if (args.id) {
        const entity = getEntityById(args.id);
        if (!entity) {
          return `ERROR: Entity '${args.id}' not found. You may call ${TOOL_NAMES.GET_ALL_ENTITIES}() to discover valid IDs.`;
        }
        return JSON.stringify(entity, null, 2);
      }
      if (args.ids && args.ids.length > 0) {
        const results = getEntitiesByIds(args.ids);
        if (results.length === 0) {
          return `ERROR: None of the requested IDs were found: [${args.ids.join(", ")}]. You may call ${TOOL_NAMES.GET_ALL_ENTITIES}() to discover valid IDs.`;
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
        return `No entities matched '${args.search}'. You may call ${TOOL_NAMES.GET_ALL_ENTITIES}() to see all entities.`;
      }
      return JSON.stringify(results, null, 2);
    }, TOOL_NAMES.QUERY_ENTITY),
  });
}

const entitySchema = z.object({
  id: z.string().describe("The unique ID of the entity to update (e.g. 'madam_vespera')."),
  shortDescription: z.string().nullish().describe("New concise label."),
  longDescription: z.string().nullish().describe("New detailed observation."),
  attributes: z
    .record(z.string(), z.string())
    .nullish()
    .describe("Physical or mental traits (merged)."),
  opinions: z
    .record(z.string(), z.string())
    .nullish()
    .describe("How this character feels about others (merged). Only valid for CHARACTER entities."),
});

export function createEditEntityTool(events: TurnEventEmitter) {
  return tool({
    title: "Edit Entity",
    description:
      "Mutate a single world entity's description, attributes, or opinions. One entity per call. Reports an error if the entity ID does not exist.",
    inputSchema: entitySchema,
    execute: wrapSafe(async (args: z.infer<typeof entitySchema>) => {
      const existing = getEntityById(args.id);
      if (!existing) {
        return `ERROR: Entity '${args.id}' not found. You may use ${TOOL_NAMES.GET_ALL_ENTITIES}() to discover valid IDs.`;
      }
      updateEntity(args);
      const changes: Record<string, unknown> = {};
      if (args.longDescription != null) changes.longDescription = args.longDescription;
      if (args.shortDescription != null) changes.shortDescription = args.shortDescription;
      if (args.attributes) changes.attributes = args.attributes;
      if (args.opinions) changes.opinions = args.opinions;
      events.emitWorldUpdate(args.id, changes);
      return `Entity with name '${existing.displayName}' (id: ${args.id}) updated.`;
    }, TOOL_NAMES.EDIT_ENTITY),
  });
}

const plotOptionSchema = z.object({
  plotId: z.string().nullable().describe("ID of the child plot, or null if not created yet."),
  triggerCondition: z.string().describe("What player action activates this branch."),
});

const plotSchema = z.object({
  title: z.string().describe("Concise title of the plot/quest."),
  description: z.string().describe("Detailed description of what this plot is about."),
  status: z
    .enum(PLOT_STATUSES)
    .optional()
    .describe("Initial status (default: PENDING)."),
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
    execute: wrapSafe(
      async (args: z.infer<typeof plotSchema>) => {
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
      },
      TOOL_NAMES.CREATE_PLOT,
    ),
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

export function createEditPlotTool(events: TurnEventEmitter) {
  return tool({
    title: "Edit Plot",
    description:
      "Update an existing plot's status, description, involved entities, or childPlots options. Only PENDING or IN_PROGRESS plots can be edited — RESOLVED plots are locked. Reports an error if the plot ID does not exist, the plot is RESOLVED, or the change would break the plot tree.",
    inputSchema: plotEditSchema,
    execute: wrapSafe(
      async (args: z.infer<typeof plotEditSchema>) => {
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
        if (args.involvedLocations !== undefined)
          changes.involvedLocations = args.involvedLocations;
        if (args.involvedCharacters !== undefined)
          changes.involvedCharacters = args.involvedCharacters;
        if (args.childPlots !== undefined) changes.childPlots = args.childPlots;
        events.emitPlotEdit(args.id, changes);

        const plot = getPlotById(args.id);
        if (!plot) return `Plot ${args.id} updated but could not be re-read.`;
        return `Plot "${plot.title}" (${args.id}) updated.`;
      },
      TOOL_NAMES.EDIT_PLOT,
    ),
  });
}

const getPlotSchema = z.object({
  id: z.string().optional().describe("Exact plot ID to fetch."),
  ids: z.array(z.string()).optional().describe("Array of plot IDs to bulk fetch."),
  status: z
    .enum(PLOT_STATUSES)
    .optional()
    .describe("Filter by status. Omit to return all plots."),
});

export function createGetPlotTool() {
  return tool({
    title: "Get Plot",
    description:
      "Retrieve plot(s): by single ID, by multiple IDs (bulk), or filter by status. Returns full plot data including childPlots.",
    inputSchema: getPlotSchema,
    execute: wrapSafe(
      async (args: z.infer<typeof getPlotSchema>) => {
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
        const filtered =
          !args.status ? all : all.filter((p) => p.status === args.status);
        if (filtered.length === 0)
          return `No plots found${args.status ? ` with status ${args.status}` : ""}.`;
        return JSON.stringify(filtered, null, 2);
      },
      TOOL_NAMES.GET_PLOT,
    ),
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
      "Optional first-person sentence for the YOU message in dialogue history after the player selects this option. Past or present tense describing what the player actually said/did (e.g. 'I tried to convince the guard to let us pass.'). If omitted, the text field is used with any [SKILL] prefix removed.",
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

export function createGenerateDialogueStepTool(_events: TurnEventEmitter) {
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
        const speakerError = checkText(msg.speaker, `generateDialogueStep messages[${i}].speaker`);
        if (speakerError) {
          errors.push(speakerError);
          break;
        }
        const textError = checkText(msg.text, `generateDialogueStep messages[${i}].text`);
        if (textError) {
          errors.push(textError);
          break;
        }
      }

      if (!args.options || args.options.length === 0) {
        errors.push(
          "Missing options — every generateDialogueStep call must include 2-5 choices for the player. Provide options that respond to the current scene.",
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
          const textError = checkText(opt.text, `generateDialogueStep options[${i}].text`);
          if (textError) {
            errors.push(textError);
            break;
          }
          if (opt.hintBefore) {
            const hintError = checkText(
              opt.hintBefore,
              `generateDialogueStep options[${i}].hintBefore`,
            );
            if (hintError) {
              errors.push(hintError);
              break;
            }
          }
          if (opt.hintAfter) {
            const hintError = checkText(
              opt.hintAfter,
              `generateDialogueStep options[${i}].hintAfter`,
            );
            if (hintError) {
              errors.push(hintError);
              break;
            }
          }
          if (opt.selectionMessage) {
            const selMsgError = checkText(
              opt.selectionMessage,
              `generateDialogueStep options[${i}].selectionMessage`,
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
        return `VALIDATION FAILED — call generateDialogueStep again with corrections:\n${errors.map((e) => `• ${e}`).join("\n")}`;
      }

      lastCallValid = true;
      return "Dialogue streamed.";
    },
  });

  return { tool: dialogueTool, wasValid: () => lastCallValid };
}

export function createAdvanceTimeTool(events: TurnEventEmitter) {
  return tool({
    title: "Advance Time",
    description:
      "Advance the in-game clock by N segments (0-11, where each segment is 2 hours). Use 0 to describe the current time without advancing. Use this when the player's action takes time — a conversation might be 0-1 segments, travel may be 2-4, a rest is 4-6. Describe why time passes in the reason field.",
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
