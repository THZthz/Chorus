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

import type { DialogueOption } from "@/types/dialogue";

// ── Text verification ──

const disallowedRe =
  /[\p{Emoji}\p{Script=Han}\p{Script=Arabic}\p{Script=Cyrillic}\p{Script=Hiragana}\p{Script=Katakana}]/u;

function isAllowedText(str: string): boolean {
  if (!disallowedRe.test(str)) return true;
  // \p{Emoji} matches ASCII digits 0-9, #, * (emoji keycap bases) — allow them
  return /^[0-9#*]$/.test(str);
}

export function checkText(value: unknown, context: string): string | null {
  const str = typeof value === "string" ? value : JSON.stringify(value);
  const disallowed = [...str].filter((c) => !isAllowedText(c));
  if (disallowed.length !== 0) {
    const unique = [...new Set(disallowed)].slice(0, 10);
    return `TEXT VERIFICATION FAILED in ${context}: disallowed characters detected [${unique.join(",")}]. Only Latin-script text and typographic punctuation (no emoji, no non-Latin scripts) is allowed. Please retry with allowed content.`;
  }
  return null;
}

// ── Error-handling wrapper ──

export function wrapSafe<T>(
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

// ── Dialogue option mapping ──

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
