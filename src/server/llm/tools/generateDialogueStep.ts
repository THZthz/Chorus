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
import type { EventEmitter } from "@/server/llm/events";
import { NOTIFICATION_TYPES } from "@/types/dialogue";
import { TOOL_NAMES, SKILL_NAMES } from "@/shared/constants";
import { checkText } from "@/server/llm/tools/shared";

const MAX_MESSAGE_TEXT_LENGTH = 500;

const messageSchema = z.object({
  speaker: z
    .string()
    .describe(
      "Name of the speaker (no '_' between words, e.g. 'LOGIC', 'Orin Fell', 'NARRATOR', 'INSTINCT', 'SORCERY')",
    ),
  type: z.enum(["INNER_VOICE", "CHARACTER", "SYSTEM", "ROLL", "NOTIFICATION"]),
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

const inputSchema = z.object({
  messages: z.array(messageSchema).describe("The sequence of messages in this dialogue step."),
  options: z.array(optionSchema).describe("The choices presented to the player."),
});

export function createGenerateDialogueStepTool(_events: EventEmitter) {
  let lastCallValid = false;

  const dialogueTool = tool({
    description:
      "Generate the narrative dialogue steps and final player choices. This is the ONLY way to communicate to the player. Options should align with the active plot's childPlots.",
    inputSchema,
    execute: async (args: z.infer<typeof inputSchema>) => {
      const errors: string[] = [];

      if (args.messages.length === 0) {
        errors.push(
          "No messages — at least 1 message is required. Provide a NARRATOR message, an NPC line, or an inner voice observation.",
        );
      }

      for (const msg of args.messages) {
        if (msg.speaker === "INNER_VOICE") {
          errors.push(
            `A message uses speaker="INNER_VOICE" — INNER_VOICE is a type, not a speaker name. Use the specific skill name as the speaker (e.g. "LOGIC", "INSTINCT", "SORCERY").`,
          );
          break;
        }
        if (
          msg.type === "INNER_VOICE" &&
          !(SKILL_NAMES as readonly string[]).includes(msg.speaker)
        ) {
          errors.push(
            `Message with type INNER_VOICE has speaker="${msg.speaker}" which is not a valid skill name. Valid skill names are: ${SKILL_NAMES.join(", ")}. Use the specific skill name as the speaker (e.g. "LOGIC", "INSTINCT", "SORCERY").`,
          );
          break;
        }
      }

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
        if (msg.text.length > MAX_MESSAGE_TEXT_LENGTH) {
          errors.push(
            `Message ${i + 1} ("${msg.speaker}") text is too long (${msg.text.length} chars, max ${MAX_MESSAGE_TEXT_LENGTH}). Shorten it to keep the UI readable.`,
          );
          break;
        }
      }

      if (!args.options || args.options.length < 2) {
        errors.push(
          `Too few options — at least 2 options are required. Every ${TOOL_NAMES.GENERATE_DIALOGUE} call must include 2-5 choices for the player.`,
        );
      } else if (args.options.length > 5) {
        errors.push(
          `Too many options (${args.options.length}) — at most 5 options are allowed. Provide 2-5 focused choices that respond to the current scene.`,
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
