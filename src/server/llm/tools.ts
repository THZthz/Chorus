import { tool } from "ai";
import { z } from "zod";
import { updateEntity } from "@/server/models/world";
import { updatePlotStatus as updatePlotStatusDb, addPlot } from "@/server/models/plot";
import type { TurnEventEmitter } from "@/server/llm/events";
import type { DialogueOption } from "@/types/dialogue";

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
      "Name of the speaker (no '_' between words, e.g. 'LOGIC', 'Madam Vespera', 'NARRATOR', 'HALF LIGHT', 'INLAND EMPIRE')",
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

export function createUpdateWorldStateTool(events: TurnEventEmitter) {
  return tool({
    title: "Update World State",
    description:
      "Commit changes to world entities (characters, locations, objects). Updates descriptions, attributes, or opinions directly in the database.",
    inputSchema: z.object({
      updates: z
        .array(
          z.object({
            id: z
              .string()
              .describe("The unique ID of the entity to update (e.g., 'madam_vespera')."),
            longDescription: z.string().nullish().describe("New detailed observation."),
            shortDescription: z.string().nullish().describe("New concise label."),
            attributes: z
              .record(z.string(), z.string())
              .nullish()
              .describe("Physical or mental traits."),
            opinions: z
              .record(z.string(), z.string())
              .nullish()
              .describe("How they feel about the player or others."),
          }),
        )
        .describe("State changes to persist in the world memory."),
    }),
    execute: async (args: {
      updates: Array<{
        id: string;
        longDescription?: string | null;
        shortDescription?: string | null;
        attributes?: Record<string, string> | null;
        opinions?: Record<string, string> | null;
      }>;
    }) => {
      for (const u of args.updates) {
        updateEntity(u);
        events.emitWorldUpdate(u.id, {
          ...(u.longDescription !== undefined ? { longDescription: u.longDescription } : {}),
          ...(u.shortDescription !== undefined ? { shortDescription: u.shortDescription } : {}),
          ...(u.attributes ? { attributes: u.attributes } : {}),
          ...(u.opinions ? { opinions: u.opinions } : {}),
        });
      }
      return `Updated ${args.updates.length} entities.`;
    },
  });
}

export function createUpdatePlotStatusTool(events: TurnEventEmitter) {
  return tool({
    title: "Update Plot Status",
    description:
      "Update the status of an existing plot (e.g., to IN_PROGRESS or RESOLVED). Commits directly to the database.",
    inputSchema: z.object({
      id: z.string().describe("The ID of the plot to update."),
      status: z
        .enum(["PENDING", "IN_PROGRESS", "RESOLVED"])
        .describe("The new status of the plot."),
    }),
    execute: async (args: { id: string; status: string }) => {
      updatePlotStatusDb(args.id, args.status);
      events.emitPlotUpdate(args.id, args.status);
      return `Plot ${args.id} status updated to ${args.status}.`;
    },
  });
}

export function createCreatePlotTool(events: TurnEventEmitter) {
  return tool({
    title: "Create Plot",
    description: "Create a new plot/quest. Commits directly to the database.",
    inputSchema: z.object({
      title: z.string().describe("Concise title of the plot/quest."),
      description: z.string().describe("Detailed description of what the plot is about."),
      triggerCondition: z
        .string()
        .describe("The specific condition or scene that triggers this plot."),
    }),
    execute: async (args: { title: string; description: string; triggerCondition: string }) => {
      const plotId = `plot_${Date.now()}`;
      addPlot({
        id: plotId,
        title: args.title,
        description: args.description,
        triggerCondition: args.triggerCondition,
      });
      events.emitPlotCreate(plotId, args.title);
      return `Plot created: ${args.title} (${plotId}).`;
    },
  });
}

export function createGenerateDialogueStepTool(_events: TurnEventEmitter) {
  let lastCallValid = false;

  const dialogueTool = tool({
    description:
      "Generate the narrative dialogue steps and final player choices. This is the ONLY way to communicate to the player.",
    inputSchema: z.object({
      messages: z.array(messageSchema).describe("The sequence of messages in this dialogue step."),
      options: z.array(optionSchema).optional().describe("The choices presented to the player."),
    }),
    execute: async (args) => {
      const errors: string[] = [];

      for (const msg of args.messages) {
        if (msg.speaker === "INNER_VOICE") {
          errors.push(
            `A message uses speaker="INNER_VOICE" — INNER_VOICE is a type, not a speaker name. Use the specific skill name as the speaker (e.g. "LOGIC", "HALF LIGHT", "INLAND EMPIRE").`,
          );
          break;
        }
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
