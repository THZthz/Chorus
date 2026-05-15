/**
 * Chorus — cinematic RPG-style dialogue engine
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
      `
When isCorrection is true: the 0-based index of the message to correct (shown in the validation error).
Omit when generating fresh.`.trim(),
    ),
  speaker: z
    .string()
    .max(60)
    .describe(
      "Name of the speaker (no '_' between words, e.g. 'LOGIC', 'Orin Fell', 'NARRATOR', 'INSTINCT', 'SORCERY').",
    ),
  type: z.enum(SPEAKER_TYPES.filter((type) => type !== "YOU") as Exclude<SpeakerType, "YOU">[]),
  text: z.string().max(MAX_MESSAGE_TEXT_LENGTH).describe("The dialogue text, supports markdown."),
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
      `
When isCorrection is true: the 0-based index of the option to correct (shown in the validation error).
Omit when generating fresh.`.trim(),
    ),
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
      skill: z.enum(SKILL_NAMES).describe("The skill to check (e.g. 'LOGIC')."),
      difficulty: z.number().describe("Numerical difficulty (e.g. 10)."),
      difficultyText: z.string().max(30).describe("Textual difficulty (e.g. 'Challenging')."),
      diceCount: z.number().default(2),
      conditions: z
        .array(
          z.object({
            expression: z
              .string()
              .max(100)
              .describe(
                "JS expression e.g. 'success', 'total - statBonus > difficulty' or 'total < difficulty'.",
              ),
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
    .optional()
    .describe(
      `
The sequence of messages in this dialogue step.
Required for fresh calls; omit during corrections if only fixing options.
If you fixing invalid messages, make sure your include "index" field to precisely repair the corresponding messages.`.trim(),
    ),
  options: z
    .array(optionSchema)
    .optional()
    .describe(
      `
The choices presented to the player.
Required for fresh calls.
Omit during corrections if only fixing options.
If you fixing invalid options, make sure your include "index" field to precisely repair the corresponding options.`.trim(),
    ),
  isCorrection: z
    .boolean()
    .optional()
    .default(false)
    .describe(
      `
Set to true when correcting specific validation errors from a previous failed call.
Only include the failing messages/options — set their "index" field to the index shown in the error.
Valid items are preserved automatically.
You can omit messages or options if only the other needs correction.`.trim(),
    ),
});

type DialogueMessage = z.infer<typeof messageSchema>;
type DialogueOpt = z.infer<typeof optionSchema>;
type DialogueArgs = z.infer<typeof inputSchema>;

interface ValidationResult {
  errors: string[];
}

function validateDialogueArgs(args: DialogueArgs): ValidationResult {
  const errors: string[] = [];

  const messages = args.messages ?? [];
  const options = args.options ?? [];

  if (messages.length === 0) {
    errors.push(
      "No messages — at least 1 message is required. Provide a NARRATOR message, an NPC line, or an inner voice observation.",
    );
  }

  // Collect ALL INNER_VOICE errors in one pass (no break)
  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];
    if (msg.speaker === "INNER_VOICE") {
      errors.push(
        `A message uses speaker="INNER_VOICE" — INNER_VOICE is a type, not a speaker name. Use the specific skill name as the speaker (e.g. "LOGIC", "INSTINCT", "SORCERY").`,
      );
    }
    if (msg.type === "INNER_VOICE" && !(SKILL_NAMES as readonly string[]).includes(msg.speaker)) {
      errors.push(
        `Message with type INNER_VOICE has speaker="${msg.speaker}" which is not a valid skill name. Valid skill names are: ${SKILL_NAMES.join(", ")}. Use the specific skill name as the speaker.`,
      );
    }
  }

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];
    const speakerError = checkText(
      msg.speaker,
      `${TOOL_NAMES.GENERATE_DIALOGUE} messages[${i}].speaker`,
    );
    if (speakerError) {
      errors.push(speakerError);
    }
    const textError = checkText(msg.text, `${TOOL_NAMES.GENERATE_DIALOGUE} messages[${i}].text`);
    if (textError) {
      errors.push(textError);
    }
    if (msg.text.length > MAX_MESSAGE_TEXT_LENGTH) {
      errors.push(
        `Message ${i + 1} ("${msg.speaker}") text is too long (${msg.text.length} chars, max ${MAX_MESSAGE_TEXT_LENGTH}). Shorten it to keep the UI readable.`,
      );
    }
  }

  if (options.length < 2) {
    errors.push(
      `Too few options — at least 2 options are required. Every ${TOOL_NAMES.GENERATE_DIALOGUE} call must include 2-5 choices for the player.`,
    );
  } else if (options.length > 5) {
    errors.push(
      `Too many options (${options.length}) — at most 5 options are allowed. Provide 2-5 focused choices that respond to the current scene.`,
    );
  }

  for (let i = 0; i < options.length; i++) {
    const opt = options[i];
    if (opt.check && opt.hintBefore) {
      errors.push(
        `Option ${i + 1} has both a skill check and hintBefore. The skill check already renders the skill name — omit hintBefore for this option.`,
      );
    }
    const textError = checkText(opt.text, `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].text`);
    if (textError) {
      errors.push(textError);
    }
    if (opt.hintBefore) {
      const hintError = checkText(
        opt.hintBefore,
        `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].hintBefore`,
      );
      if (hintError) {
        errors.push(hintError);
      }
    }
    if (opt.hintAfter) {
      const hintError = checkText(
        opt.hintAfter,
        `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].hintAfter`,
      );
      if (hintError) {
        errors.push(hintError);
      }
    }
    if (opt.selectionMessage) {
      const selMsgError = checkText(
        opt.selectionMessage,
        `${TOOL_NAMES.GENERATE_DIALOGUE} options[${i}].selectionMessage`,
      );
      if (selMsgError) {
        errors.push(selMsgError);
      }
    }
  }

  return { errors };
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
  skipPersistIndices?: Set<number>,
): Promise<string> {
  const result = validateDialogueArgs(args);

  if (result.errors.length > 0) {
    onValidChange?.(false);
    return [
      "VALIDATION FAILED ",
      "(isCorrection" + (isCorrection ? "true" : "false") + ")\n",
      result.errors.map((e) => "- " + e).join("\n"),
      "Call generateDialogueStep again with isCorrection: true. ",
      "Only send the failing items listed in 'failures' — set each item's 'index' field to the index shown. ",
      "Valid items are preserved from the previous call automatically (do NOT copy them).",
    ].join("");
  }

  onValidChange?.(true);

  if (persistMessage) {
    const messages = args.messages ?? [];
    let persisted = 0;
    for (let i = 0; i < messages.length; i++) {
      if (skipPersistIndices?.has(i)) continue;
      const msg = messages[i];
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
          `[${TOOL_NAMES.GENERATE_DIALOGUE}] Failed to persist message from "${msg.speaker}":`,
          err,
        );
      }
    }
    return (
      (isCorrection ? `Correction applied — ` : `Dialogue successfully streamed — `) +
      `${persisted} message(s) persisted, ${(args.options ?? []).length} option(s) received.`
    );
  }

  return (
    (isCorrection
      ? "Correction applied — dialogue successfully streamed. "
      : "Dialogue successfully streamed. ") +
    `${(args.messages ?? []).length} message(s) received, ${(args.options ?? []).length} option(s) received.`
  );
}

export function createGenerateDialogueStepTool(persistMessage?: PersistMessageFn) {
  let lastCallValid = false;
  let lastCallMessages: DialogueMessage[] = [];
  let lastCallOptions: DialogueOpt[] = [];
  let lastPersistedCount = 0;

  const dialogueTool = tool({
    title: TOOL_NAMES.GENERATE_DIALOGUE,
    description: `
Generate the narrative dialogue steps and final player choices.
This is the ONLY way to communicate to the player.
Options should align with the active plot's childPlots.

When a previous call fails validation, call again with isCorrection: true.
Only include the messages/options that need fixing — set their "index" field to the index shown in the validation error.
Valid items are preserved from the previous call automatically.
You do NOT need to copy them.
`.trim(),
    inputSchema,
    execute: async (args: DialogueArgs) => {
      const isCorrection = args.isCorrection ?? false;

      const baseEmpty = lastCallMessages.length === 0 && lastCallOptions.length === 0;

      // Auto-merge: when correcting, start from stored base and patch in the corrections.
      // Skip merge if the stored base is empty (previous call failed Zod validation before
      // reaching execute, or the state was reset for a new turn). Items with index replace
      // the existing item at that position; items without index are appended.
      const replacedMessageIndices = new Set<number>();
      if (isCorrection && !baseEmpty) {
        const mergedMessages = [...lastCallMessages];
        if (args.messages) {
          for (const msg of args.messages) {
            if (msg.index !== undefined && msg.index < mergedMessages.length) {
              mergedMessages[msg.index] = msg;
              replacedMessageIndices.add(msg.index);
            } else {
              mergedMessages.push(msg);
            }
          }
        }

        const mergedOptions = [...lastCallOptions];
        if (args.options) {
          for (const opt of args.options) {
            if (opt.index !== undefined && opt.index < mergedOptions.length) {
              mergedOptions[opt.index] = opt;
            } else {
              mergedOptions.push(opt);
            }
          }
        }

        args = { ...args, messages: mergedMessages, options: mergedOptions };
      }

      // Messages that were already persisted from a previous call and not
      // replaced in this correction should be skipped.
      const skipPersist = new Set<number>();
      if (isCorrection && lastPersistedCount > 0) {
        for (let i = 0; i < lastPersistedCount; i++) {
          if (!replacedMessageIndices.has(i)) skipPersist.add(i);
        }
      }

      const result = await executeAndPersist(
        args, isCorrection, persistMessage,
        (valid) => { lastCallValid = valid; },
        skipPersist.size > 0 ? skipPersist : undefined,
      );

      // Store this call's args for potential future correction within the same turn.
      lastCallMessages = args.messages ?? [];
      lastCallOptions = args.options ?? [];
      lastPersistedCount = (args.messages ?? []).length;

      return result;
    },
  });

  return {
    tool: dialogueTool,
    wasValid: () => lastCallValid,
    resetForTurn: () => {
      lastCallValid = false;
      lastCallMessages = [];
      lastCallOptions = [];
      lastPersistedCount = 0;
    },
  };
}
