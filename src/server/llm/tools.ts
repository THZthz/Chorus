import { tool } from "ai";
import { z } from "zod";
import {
  updateEntity,
  getEntityById,
  getAllEntitySummaries,
  searchEntities,
} from "@/server/models/world";
import { addPlot, updatePlot, getPlotById, getPlotsByIds, getAllPlots } from "@/server/models/plot";
import type { PlotOption } from "@/types/plot";
import type { TurnEventEmitter } from "@/server/llm/events";
import type { DialogueOption } from "@/types/dialogue";

// ── ASCII verification ──

function isAscii(str: string): boolean {
  return /^[\x00-\x7F]*$/.test(str);
}

function checkAscii(value: unknown, context: string): string | null {
  const str = typeof value === "string" ? value : JSON.stringify(value);
  if (!isAscii(str)) {
    const nonAscii = [...str].filter((c) => c.charCodeAt(0) > 127);
    const unique = [...new Set(nonAscii)].slice(0, 10);
    return `ASCII VERIFICATION FAILED in ${context}: non-ASCII characters detected [${unique.join(" ")}]. Only plain ASCII (English text, no emoji, no other languages) is allowed. Please retry with ASCII-only content.`;
  }
  return null;
}

// ── Error-handling wrapper ──

function wrapSafe<T>(
  fn: (args: T) => Promise<string>,
  toolName: string,
): (args: T) => Promise<string> {
  return async (args: T) => {
    const inputError = checkAscii(args, `${toolName} input`);
    if (inputError) return inputError;

    try {
      const result = await fn(args);
      const outputError = checkAscii(result, `${toolName} output`);
      if (outputError) return outputError;
      return result;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(`[${toolName}] execute error:`, err);
      return `ERROR: Tool "${toolName}" failed unexpectedly: ${msg}. Please retry or use a different approach.`;
    }
  };
}

// ── Shared schemas ──

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

const messageSchema = z.object({
  speaker: z
    .string()
    .describe(
      "Name of the speaker (no '_' between words, e.g. 'LOGIC', 'Orin Fell', 'NARRATOR', 'INSTINCT', 'SORCERY')",
    ),
  type: z.enum(["YOU", "INNER_VOICE", "CHARACTER", "SYSTEM", "NOTIFICATION"]),
  text: z.string().describe("The dialogue text, supports markdown."),
  metadata: z
    .object({
      notificationType: z.enum(["XP", "TASK", "ITEM"]).optional(),
    })
    .optional(),
});

const optionSchema = z.object({
  text: z.string().describe("The text shown to the player."),
  id: z.string().optional(),
  hintBefore: z.string().optional().describe("Hint shown before the text e.g. [Logic]"),
  hintAfter: z.string().optional().describe("Hint shown after the text e.g. [Red Check]"),
  isAiTrigger: z
    .boolean()
    .optional()
    .describe("Must be true if user selection triggers a new AI response."),
  check: skillCheckSchema.optional(),
});

const plotOptionSchema = z.object({
  plotId: z.string().nullable().describe("ID of the child plot, or null if not created yet."),
  triggerCondition: z.string().describe("What player action activates this branch."),
});

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
    hintBefore: o.hintBefore as string | undefined,
    hintAfter: o.hintAfter as string | undefined,
    isAiTrigger: (o.isAiTrigger as boolean) ?? true,
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
    description:
      "Returns the id, displayName, type, and shortDescription of all world entities. Use this to discover what exists before calling queryEntity for full details.",
    inputSchema: z.object({
      type: z
        .enum(["CHARACTER", "LOCATION", "OBJECT"])
        .optional()
        .describe("Optional filter by entity type."),
    }),
    execute: wrapSafe(async (args: { type?: "CHARACTER" | "LOCATION" | "OBJECT" }) => {
      const summaries = getAllEntitySummaries(args.type);
      if (summaries.length === 0) return "No entities found.";
      return JSON.stringify(summaries, null, 2);
    }, "getAllEntitiesName"),
  });
}

export function createQueryEntityTool() {
  return tool({
    title: "Query Entity",
    description:
      "Get full details of a world entity. Provide either an exact id or a search term (case-insensitive match on name/description).",
    inputSchema: z.object({
      id: z.string().optional().describe("Exact entity ID (e.g. 'madam_vespera')."),
      search: z
        .string()
        .optional()
        .describe("Text to search for in entity names/descriptions (up to 5 results)."),
    }),
    execute: wrapSafe(async (args: { id?: string; search?: string }) => {
      if (!args.id && !args.search) {
        return "ERROR: Provide either 'id' for exact lookup or 'search' for text search.";
      }
      if (args.id) {
        const entity = getEntityById(args.id);
        if (!entity) {
          return `ERROR: Entity '${args.id}' not found. Call getAllEntitiesName() to discover valid IDs.`;
        }
        return JSON.stringify(entity, null, 2);
      }
      const results = searchEntities(args.search!);
      if (results.length === 0) {
        return `No entities matched '${args.search}'. Call getAllEntitiesName() to see all entities.`;
      }
      return JSON.stringify(results, null, 2);
    }, "queryEntity"),
  });
}

export function createEditEntityTool(events: TurnEventEmitter) {
  return tool({
    title: "Edit Entity",
    description:
      "Mutate a single world entity's description, attributes, or opinions. One entity per call. Reports an error if the entity ID does not exist.",
    inputSchema: z.object({
      id: z.string().describe("The unique ID of the entity to update (e.g. 'madam_vespera')."),
      longDescription: z.string().nullish().describe("New detailed observation."),
      shortDescription: z.string().nullish().describe("New concise label."),
      attributes: z
        .record(z.string(), z.string())
        .nullish()
        .describe("Physical or mental traits (merged)."),
      opinions: z
        .record(z.string(), z.string())
        .nullish()
        .describe(
          "How this character feels about others (merged). Only valid for CHARACTER entities.",
        ),
    }),
    execute: wrapSafe(async (args: {
      id: string;
      longDescription?: string | null;
      shortDescription?: string | null;
      attributes?: Record<string, string> | null;
      opinions?: Record<string, string> | null;
    }) => {
      const existing = getEntityById(args.id);
      if (!existing) {
        return `ERROR: Entity '${args.id}' not found. Use getAllEntitiesName() to discover valid IDs.`;
      }
      updateEntity(args);
      const changes: Record<string, unknown> = {};
      if (args.longDescription != null) changes.longDescription = args.longDescription;
      if (args.shortDescription != null) changes.shortDescription = args.shortDescription;
      if (args.attributes) changes.attributes = args.attributes;
      if (args.opinions) changes.opinions = args.opinions;
      events.emitWorldUpdate(args.id, changes);
      return `Entity '${existing.displayName}' (${args.id}) updated.`;
    }, "editEntity"),
  });
}

export function createCreatePlotTool(events: TurnEventEmitter) {
  return tool({
    title: "Create Plot",
    description:
      "Create a new plot node in the story tree. If this is the first plot, omit parentPlotId to create the root. Otherwise provide parentPlotId and parentOptionId (index into parent's childPlots array) to link it into the tree. The parent's childPlots[parentOptionId].plotId will be auto-updated.",
    inputSchema: z.object({
      title: z.string().describe("Concise title of the plot/quest."),
      description: z.string().describe("Detailed description of what this plot is about."),
      status: z
        .enum(["PENDING", "IN_PROGRESS", "RESOLVED"])
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
    }),
    execute: wrapSafe(async (args: {
      title: string;
      description: string;
      status?: "PENDING" | "IN_PROGRESS" | "RESOLVED";
      involvedLocations?: string[];
      involvedCharacters?: string[];
      parentPlotId?: string | null;
      parentOptionId?: number | null;
      childPlots?: PlotOption[];
    }) => {
      const plotId = `plot_${Date.now()}`;
      const result = addPlot({
        id: plotId,
        title: args.title,
        description: args.description,
        status: args.status ?? "PENDING",
        involvedLocations: args.involvedLocations ?? [],
        involvedCharacters: args.involvedCharacters ?? [],
        parentPlotId: args.parentPlotId ?? null,
        parentOptionId: args.parentOptionId ?? null,
        childPlots: args.childPlots ?? [],
      });

      if (result.ok === false) {
        return `ERROR: ${result.error}`;
      }

      events.emitPlotCreate(plotId, args.title, args.parentPlotId ?? null);
      return `Plot created: "${args.title}" (${plotId}).`;
    }, "createPlot"),
  });
}

export function createEditPlotTool(events: TurnEventEmitter) {
  return tool({
    title: "Edit Plot",
    description:
      "Update an existing plot's status, description, involved entities, or childPlots options. Only PENDING or IN_PROGRESS plots can be edited — RESOLVED plots are locked. Reports an error if the plot ID does not exist, the plot is RESOLVED, or the change would break the plot tree.",
    inputSchema: z.object({
      id: z.string().describe("The ID of the plot to update."),
      status: z
        .enum(["PENDING", "IN_PROGRESS", "RESOLVED"])
        .optional()
        .describe("New status for the plot."),
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
    }),
    execute: wrapSafe(async (args: {
      id: string;
      status?: "PENDING" | "IN_PROGRESS" | "RESOLVED";
      description?: string;
      involvedLocations?: string[];
      involvedCharacters?: string[];
      childPlots?: PlotOption[];
    }) => {
      const result = updatePlot(args.id, {
        status: args.status,
        description: args.description,
        involvedLocations: args.involvedLocations,
        involvedCharacters: args.involvedCharacters,
        childPlots: args.childPlots,
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
    }, "editPlot"),
  });
}

export function createGetPlotTool() {
  return tool({
    title: "Get Plot",
    description:
      "Retrieve plot(s): by single ID, by multiple IDs (bulk), or filter by status. Returns full plot data including childPlots.",
    inputSchema: z.object({
      id: z.string().optional().describe("Exact plot ID to fetch."),
      ids: z.array(z.string()).optional().describe("Array of plot IDs to bulk fetch."),
      status: z
        .enum(["PENDING", "IN_PROGRESS", "RESOLVED", "ALL"])
        .optional()
        .describe("Filter by status. Omit to return all plots."),
    }),
    execute: wrapSafe(async (args: {
      id?: string;
      ids?: string[];
      status?: "PENDING" | "IN_PROGRESS" | "RESOLVED" | "ALL";
    }) => {
      if (args.id && args.ids) {
        return "ERROR: Provide either 'id' for a single plot or 'ids' for bulk fetch, not both.";
      }
      if (args.id) {
        const plot = getPlotById(args.id);
        if (!plot) {
          return `ERROR: Plot '${args.id}' not found. Use getPlot() without an id to list all plots.`;
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
        !args.status || args.status === "ALL" ? all : all.filter((p) => p.status === args.status);
      if (filtered.length === 0)
        return `No plots found${args.status ? ` with status ${args.status}` : ""}.`;
      return JSON.stringify(filtered, null, 2);
    }, "getPlot"),
  });
}

export function createGenerateDialogueStepTool(_events: TurnEventEmitter) {
  let lastCallValid = false;

  const dialogueTool = tool({
    description:
      "Generate the narrative dialogue steps and final player choices. This is the ONLY way to communicate to the player. Options should align with the active plot's childPlots.",
    inputSchema: z.object({
      messages: z.array(messageSchema).describe("The sequence of messages in this dialogue step."),
      options: z.array(optionSchema).optional().describe("The choices presented to the player."),
    }),
    execute: async (args) => {
      const errors: string[] = [];

      for (const msg of args.messages) {
        if (msg.speaker === "INNER_VOICE") {
          errors.push(
            `A message uses speaker="INNER_VOICE" — INNER_VOICE is a type, not a speaker name. Use the specific skill name as the speaker (e.g. "LOGIC", "INSTINCT", "SORCERY").`,
          );
          break;
        }
      }

      // ASCII verification on messages
      for (let i = 0; i < args.messages.length; i++) {
        const msg = args.messages[i];
        const speakerError = checkAscii(msg.speaker, `generateDialogueStep messages[${i}].speaker`);
        if (speakerError) { errors.push(speakerError); break; }
        const textError = checkAscii(msg.text, `generateDialogueStep messages[${i}].text`);
        if (textError) { errors.push(textError); break; }
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

        // ASCII verification on options
        for (let i = 0; i < args.options.length; i++) {
          const opt = args.options[i];
          const textError = checkAscii(opt.text, `generateDialogueStep options[${i}].text`);
          if (textError) { errors.push(textError); break; }
          if (opt.hintBefore) {
            const hintError = checkAscii(opt.hintBefore, `generateDialogueStep options[${i}].hintBefore`);
            if (hintError) { errors.push(hintError); break; }
          }
          if (opt.hintAfter) {
            const hintError = checkAscii(opt.hintAfter, `generateDialogueStep options[${i}].hintAfter`);
            if (hintError) { errors.push(hintError); break; }
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
