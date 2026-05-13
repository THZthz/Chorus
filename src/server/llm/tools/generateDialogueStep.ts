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
import { NOTIFICATION_TYPES, SPEAKER_TYPES, SpeakerType } from "@/types/dialogue";
import { TOOL_NAMES, SKILL_NAMES } from "@/shared/constants";
import { checkText } from "@/server/llm/tools/shared";

const MAX_MESSAGE_TEXT_LENGTH = 500;

const messageSchema = z.object({
  index: z
    .number()
    .int()
    .min(0)
    .optional()
    .describe(
      "When isCorrection is true: the 0-based index of the message to correct (shown in the validation error). Omit when generating fresh.",
    ),
  speaker: z
    .string()
    .max(60)
    .describe(
      "Name of the speaker (no '_' between words, e.g. 'LOGIC', 'Orin Fell', 'NARRATOR', 'INSTINCT', 'SORCERY')",
    ),
  type: z.enum(
    SPEAKER_TYPES.filter((type) => type !== "YOU") as Exclude<SpeakerType, "YOU">[],
  ),
  text: z.string().max(500).describe("The dialogue text, supports markdown."),
  metadata: z
    .object({
      notificationType: z.enum(NOTIFICATION_TYPES).optional(),
    })
    .optional(),
});

const optionSchema = z.object({
  index: z
    .number()
    .int()
    .min(0)
    .optional()
    .describe(
      "When isCorrection is true: the 0-based index of the option to correct (shown in the validation error). Omit when generating fresh.",
    ),
  id: z.string().max(40).optional(),
  text: z
    .string()
    .max(200)
    .describe("Short imperative button label (e.g. 'Try to convince the guard')."),
  selectionMessage: z
    .string()
    .max(300)
    .optional()
    .describe(
      `
Optional sentence for the YOU message in dialogue history after the player selects this option.
Write in past or present tense WITHOUT the pronoun 'I' — the system prefixes with 'You:' automatically
(e.g. 'Tried to convince the guard to let us pass.' reads as 'You: Tried to convince...').
Using 'I' would produce the awkward 'You: I tried...'.
If omitted, the text field is used with any [SKILL] prefix removed.`.trim(),
    ),
  hintBefore: z
    .string()
    .max(50)
    .optional()
    .describe("Hint shown before the text, e.g. [Logic]. Do not overuse it."),
  hintAfter: z
    .string()
    .max(50)
    .optional()
    .describe("Hint shown after the text, e.g. [Check]. Do not overuse it."),
  check: z
    .object({
      skill: z.enum(SKILL_NAMES).describe("The skill to check (e.g. 'LOGIC')"),
      difficulty: z.number().describe("Numerical difficulty (e.g. 10)"),
      difficultyText: z.string().max(30).describe("Textual difficulty (e.g. 'Challenging')"),
      diceCount: z.number().default(2),
      conditions: z
        .array(
          z.object({
            expression: z
              .string()
              .max(100)
              .describe("JS expression e.g. 'success' or 'total < difficulty'"),
            label: z.string().max(100).optional(),
            color: z.string().max(30).optional(),
          }),
        )
        .describe("Outcome conditions."),
    })
    .optional(),
});

const inputSchema = z.object({
  messages: z
    .array(messageSchema)
    .describe("The sequence of messages in this dialogue step."),
  options: z
    .array(optionSchema)
    .describe("The choices presented to the player."),
  isCorrection: z
    .boolean()
    .optional()
    .default(false)
    .describe(
      "Set to true when correcting specific validation errors from a previous failed call. Only include the failing messages/options — set their 'index' field to the index shown in the error. Valid items are preserved automatically.",
    ),
});

type DialogueArgs = z.infer<typeof inputSchema>;

interface ValidationResult {
  errors: string[];
  /** 0-based indices of messages that passed individual validation */
  validMessageIndices: Set<number>;
  /** 0-based indices of options that passed individual validation */
  validOptionIndices: Set<number>;
}

function validateDialogueArgs(args: DialogueArgs): ValidationResult {
  const errors: string[] = [];
  const validMessageIndices = new Set<number>();
  const validOptionIndices = new Set<number>();

  // Mark all indices as potentially valid, then remove failing ones
  for (let i = 0; i < args.messages.length; i++) validMessageIndices.add(i);
  if (args.options) {
    for (let i = 0; i < args.options.length; i++) validOptionIndices.add(i);
  }

  if (args.messages.length === 0) {
    errors.push(
      "No messages — at least 1 message is required. Provide a NARRATOR message, an NPC line, or an inner voice observation.",
    );
    validMessageIndices.clear();
  }

  // Collect ALL INNER_VOICE errors in one pass (no break)
  for (let i = 0; i < args.messages.length; i++) {
    const msg = args.messages[i];
    if (msg.speaker === "INNER_VOICE") {
      errors.push(
        `A message uses speaker="INNER_VOICE" — INNER_VOICE is a type, not a speaker name. Use the specific skill name as the speaker (e.g. "LOGIC", "INSTINCT", "SORCERY").`,
      );
      validMessageIndices.delete(i);
    }
    if (
      msg.type === "INNER_VOICE" &&
      !(SKILL_NAMES as readonly string[]).includes(msg.speaker)
    ) {
      errors.push(
        `Message with type INNER_VOICE has speaker="${msg.speaker}" which is not a valid skill name. Valid skill names are: ${SKILL_NAMES.join(", ")}. Use the specific skill name as the speaker (e.g. "LOGIC", "INSTINCT", "SORCERY").`,
      );
      validMessageIndices.delete(i);
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
      validMessageIndices.delete(i);
    }
    const textError = checkText(
      msg.text,
      `${TOOL_NAMES.GENERATE_DIALOGUE} messages[${i}].text`,
    );
    if (textError) {
      errors.push(textError);
      validMessageIndices.delete(i);
    }
    if (msg.text.length > MAX_MESSAGE_TEXT_LENGTH) {
      errors.push(
        `Message ${i + 1} ("${msg.speaker}") text is too long (${msg.text.length} chars, max ${MAX_MESSAGE_TEXT_LENGTH}). Shorten it to keep the UI readable.`,
      );
      validMessageIndices.delete(i);
    }
  }

  if (!args.options || args.options.length < 2) {
    errors.push(
      `Too few options — at least 2 options are required. Every ${TOOL_NAMES.GENERATE_DIALOGUE} call must include 2-5 choices for the player.`,
    );
    validOptionIndices.clear();
  } else if (args.options.length > 5) {
    errors.push(
      `Too many options (${args.options.length}) — at most 5 options are allowed. Provide 2-5 focused choices that respond to the current scene.`,
    );
    validOptionIndices.clear();
  }

  if (args.options) {
    // Skip per-option checks when count check already cleared all indices
    if (validOptionIndices.size > 0) {
      for (let i = 0; i < args.options.length; i++) {
        const opt = args.options[i];
        if (opt.check && opt.hintBefore) {
          errors.push(
            `Option ${i + 1} has both a skill check and hintBefore. The skill check already renders the skill name — omit hintBefore for this option.`,
          );
          validOptionIndices.delete(i);
        }
      }
    }

    if (validOptionIndices.size > 0) {
      for (let i = 0; i < args.options.length; i++) {
        const opt = args.options[i];
        const textError = checkText(
          opt.text,
          `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].text`,
        );
        if (textError) {
          errors.push(textError);
          validOptionIndices.delete(i);
        }
        if (opt.hintBefore) {
          const hintError = checkText(
            opt.hintBefore,
            `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].hintBefore`,
          );
          if (hintError) {
            errors.push(hintError);
            validOptionIndices.delete(i);
          }
        }
        if (opt.hintAfter) {
          const hintError = checkText(
            opt.hintAfter,
            `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].hintAfter`,
          );
          if (hintError) {
            errors.push(hintError);
            validOptionIndices.delete(i);
          }
        }
        if (opt.selectionMessage) {
          const selMsgError = checkText(
            opt.selectionMessage,
            `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].selectionMessage`,
          );
          if (selMsgError) {
            errors.push(selMsgError);
            validOptionIndices.delete(i);
          }
        }
      }
    }
  }

  return { errors, validMessageIndices, validOptionIndices };
}

function formatValidationFailure(
  args: DialogueArgs,
  result: ValidationResult,
  isCorrection: boolean,
): string {
  const lines: string[] = [];
  if (!isCorrection) {
    lines.push(
      "VALIDATION FAILED — call generateDialogueStep again with isCorrection: true. Only send the failing items (with their index field set):",
    );
  } else {
    lines.push(
      "VALIDATION FAILED — call generateDialogueStep again with isCorrection: true and further corrections (only send the still-failing items with their index):",
    );
  }

  // Echo valid content — encourages LLM to keep these verbatim
  const validMsgs = [...result.validMessageIndices].sort((a, b) => a - b);
  const validOpts = [...result.validOptionIndices].sort((a, b) => a - b);
  if (validMsgs.length > 0) {
    lines.push(`\nValid messages (keep exactly as-is):`);
    for (const i of validMsgs) {
      const msg = args.messages[i];
      const truncatedText =
        msg.text.length > 100 ? msg.text.slice(0, 100) + "..." : msg.text;
      lines.push(
        `  messages[${i}]: speaker="${msg.speaker}" type="${msg.type}" text="${truncatedText}"`,
      );
    }
  }
  if (validOpts.length > 0) {
    lines.push(`\nValid options (keep exactly as-is):`);
    for (const i of validOpts) {
      const opt = args.options![i];
      const parts: string[] = [`options[${i}]:`];
      parts.push(`text="${opt.text}"`);
      if (opt.hintBefore) parts.push(`hintBefore="${opt.hintBefore}"`);
      if (opt.hintAfter) parts.push(`hintAfter="${opt.hintAfter}"`);
      lines.push(`  ${parts.join(" ")}`);
    }
  }

  // Report errors
  lines.push(`\nErrors to fix:`);
  for (const e of result.errors) {
    lines.push(`• ${e}`);
  }

  lines.push(
    `\nWhen retrying with isCorrection: true, ONLY send the failing items listed above — set each item's "index" field to the index shown. Valid items are preserved from the previous call automatically (do NOT copy them).`,
  );

  return lines.join("\n");
}

type PersistMessageFn = (msg: {
  speaker: string;
  type: string;
  text: string;
  metadata?: Record<string, unknown>;
}) => Promise<void>;

async function executeAndPersist(
  args: DialogueArgs,
  isCorrection: boolean,
  persistMessage?: PersistMessageFn,
  onValidChange?: (valid: boolean) => void,
): Promise<string> {
  const result = validateDialogueArgs(args);

  if (result.errors.length > 0) {
    onValidChange?.(false);
    return formatValidationFailure(args, result, isCorrection);
  }

  onValidChange?.(true);

  if (persistMessage) {
    let persisted = 0;
    for (const msg of args.messages) {
      try {
        await persistMessage({
          speaker: msg.speaker,
          type: msg.type,
          text: msg.text,
          metadata: msg.metadata,
        });
        persisted++;
      } catch (err) {
        console.warn(
          `[generateDialogueStep] Failed to persist message from "${msg.speaker}":`,
          err,
        );
      }
    }
    if (!isCorrection) {
      return `Dialogue successfully streamed and ${persisted} message(s) persisted.`;
    }
    return `Correction applied — ${persisted} message(s) persisted.`;
  }

  if (!isCorrection) {
    return "Dialogue successfully streamed.";
  }
  return "Correction applied — dialogue successfully streamed.";
}

export function createGenerateDialogueStepTool(
  persistMessage?: PersistMessageFn,
) {
  let lastCallValid = false;
  let lastCallMessages: DialogueArgs["messages"] = [];
  let lastCallOptions: DialogueArgs["options"] = [];

  const dialogueTool = tool({
    description: `
Generate the narrative dialogue steps and final player choices.
This is the ONLY way to communicate to the player.
Options should align with the active plot's childPlots.

When a previous call fails validation, call again with isCorrection: true.
Only include the messages/options that need fixing — set their "index" field
to the index shown in the validation error. Valid items are preserved
from the previous call automatically. You do NOT need to copy them.`.trim(),
    inputSchema,
    execute: async (args: DialogueArgs) => {
      const isCorrection = args.isCorrection ?? false;

      // Auto-merge: when correcting, start from stored base and patch in the corrections
      if (isCorrection && lastCallMessages.length > 0) {
        const mergedMessages = [...lastCallMessages];
        for (const msg of args.messages) {
          if (msg.index !== undefined && msg.index < mergedMessages.length) {
            mergedMessages[msg.index] = msg;
          }
        }

        const mergedOptions = lastCallOptions ? [...lastCallOptions] : [];
        if (args.options) {
          for (const opt of args.options) {
            if (opt.index !== undefined && opt.index < mergedOptions.length) {
              mergedOptions[opt.index] = opt;
            }
          }
        }

        args = { ...args, messages: mergedMessages, options: mergedOptions };
      }

      const result = await executeAndPersist(
        args,
        isCorrection,
        persistMessage,
        (valid) => { lastCallValid = valid; },
      );

      // Store this call's args for potential future correction (only for non-correction calls,
      // or correction calls that still had some valid items — so the LLM can iterate)
      if (!isCorrection || args.messages.length > 0) {
        lastCallMessages = args.messages;
        lastCallOptions = args.options;
      }

      return result;
    },
  });

  return { tool: dialogueTool, wasValid: () => lastCallValid };
}
